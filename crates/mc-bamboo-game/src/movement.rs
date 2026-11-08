//! 玩家移动和传送确认。
//!
//! 位置是否接受由 `player_move` 按官方 `handleMovePlayer` 决定。这里负责传送包和朝向。
//! 区块由 tick 的跟踪阶段根据新位置处理，实体同步也是每 tick 比较位置。
//!
//! Whether a position is accepted is decided in `player_move`, matching `handleMovePlayer`.
//! This module owns the teleport packet and the facing. Chunk tracking and entity sync
//! still read the position once per tick.

use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::{packet::play::PlayerPosition, text::Component};

use crate::{
    inbound::PlayerId,
    links::Links,
    player::Player,
    player_grid::PlayerGrid,
    region::{Region, RegionTick},
};

const MAX_PITCH: f32 = 90.0;
/// 一次传送多久没被确认就重发。只有收到移动包时才检查，不是定时器。
///
/// How long an unconfirmed teleport waits before it is sent again. Checked only when a move
/// packet arrives, not on a timer.
pub(crate) const TELEPORT_RESEND_TICKS: u64 = 20;

const INVALID_MOVEMENT: &str = "multiplayer.disconnect.invalid_player_movement";

/// 传送确认的状态。
///
/// 服务端传送之后，网络上可能还有客户端在传送之前发出的移动包。
/// 确认到达之前丢掉它们的位置，否则玩家会被拉回原处。
#[derive(Debug, Default)]
pub(crate) struct Teleport {
    /// 最新一次传送的 id。第一次是 1，和 `player_position` 里的相同。
    pub(crate) latest_id: i32,
    /// 有传送没被确认时，是传送之后的服务端位置。
    pub(crate) awaiting_position: Option<Vec3>,
    /// 最新一次传送发出的时刻。
    pub(crate) sent_at: RegionTick,
}

impl Teleport {
    /// 有传送还没被确认。方块放置要等确认之后才接受。
    pub(crate) fn is_awaiting(&self) -> bool {
        self.awaiting_position.is_some()
    }

    fn next_id(&mut self) -> i32 {
        self.latest_id += 1;
        if self.latest_id == i32::MAX {
            self.latest_id = 0;
        }
        self.latest_id
    }
}

/// 把玩家传送到 `target`，朝向不变。
pub(crate) fn teleport(
    player: &mut Player,
    links: &mut Links,
    grid: &mut PlayerGrid,
    now: RegionTick,
    target: Vec3,
) {
    let teleport_id = player.teleport.next_id();
    player.teleport.sent_at = now;
    player.teleport.awaiting_position = Some(target);
    player.set_position(grid, target);
    let Vec3 { x, y, z } = target;
    let packet = PlayerPosition {
        teleport_id,
        x,
        y,
        z,
        yaw: player.yaw,
        pitch: player.pitch,
        relative: 0,
    };
    links.send(player.id, &packet);
}

/// `/tp` 的一次传送。包里相对轴传差值，绝对轴传落点，朝向按算好的值。
///
/// One `/tp`. Relative axes are deltas, absolute axes are the destination, and rotation is
/// the value already computed for the packet.
pub(crate) struct CommandTeleport {
    pub absolute: Vec3,
    pub packet_x: f64,
    pub packet_y: f64,
    pub packet_z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub packet_yaw: f32,
    pub packet_pitch: f32,
    pub relative: i32,
}

pub(crate) fn teleport_command(
    player: &mut Player,
    links: &mut Links,
    grid: &mut PlayerGrid,
    now: RegionTick,
    motion: CommandTeleport,
) {
    let teleport_id = player.teleport.next_id();
    player.teleport.sent_at = now;
    player.teleport.awaiting_position = Some(motion.absolute);
    player.set_position(grid, motion.absolute);
    // 官方 `teleportSetPosition` 把算好的角度直接写上，不再包一层 `wrapDegrees`。
    //
    // Official `teleportSetPosition` stores the computed angles without another `wrapDegrees`.
    player.yaw = motion.yaw;
    player.head_yaw = motion.yaw;
    player.pitch = motion.pitch;
    player.on_ground = true;
    links.send(
        player.id,
        &PlayerPosition {
            teleport_id,
            x: motion.packet_x,
            y: motion.packet_y,
            z: motion.packet_z,
            yaw: motion.packet_yaw,
            pitch: motion.packet_pitch,
            relative: motion.relative,
        },
    );
}

/// 处理四种移动包。线上没有的字段是 `None`，保持玩家当前的值。
///
/// Handles the four move packets. A field that was not on the wire is `None` and keeps
/// the player's current value.
pub(crate) fn on_move_player(
    region: &mut Region,
    id: PlayerId,
    position: Option<Vec3>,
    rotation: Option<(f32, f32)>,
    on_ground: bool,
    horizontal_collision: bool,
) {
    crate::player_move::on_move_player(
        region,
        id,
        position,
        rotation,
        on_ground,
        horizontal_collision,
    );
}

/// 处理 `accept_teleportation`。
///
/// 客户端确认之后不再发移动包，所以确认包里的位置就是它的下一次移动。
///
/// Handles `accept_teleportation`. The client sends no further move packet, so the position
/// in the confirmation is its next movement.
pub(crate) fn on_accept_teleportation(
    region: &mut Region,
    id: PlayerId,
    teleport_id: i32,
    position: Vec3,
    rotation: (f32, f32),
) {
    crate::player_move::on_accept_teleportation(region, id, teleport_id, position, rotation);
}

/// 一个服务端 tick 收完包之后的飞行和加载检查。
///
/// Flying and client-load checks after a server tick has finished its packets.
pub(crate) fn finish_tick(region: &mut Region) {
    crate::player_move::finish_tick(region);
}

/// `client_tick_end`。标记只由这个包清掉，服务端 tick 不清。
///
/// `client_tick_end`. Only this packet clears the flag; the server tick does not.
pub(crate) fn on_client_tick_end(region: &mut Region, id: PlayerId) {
    crate::player_move::on_client_tick_end(region, id);
}

/// `player_loaded`。
///
/// `player_loaded`.
pub(crate) fn on_player_loaded(region: &mut Region, id: PlayerId) {
    crate::player_move::on_player_loaded(region, id);
}

pub(crate) fn invalid_movement() -> Component {
    translated(INVALID_MOVEMENT)
}

pub(crate) fn translated(key: &str) -> Component {
    Component::translate(key, [])
}

/// 位置是 NaN，或者朝向不是有限值。±无穷的位置不算非法，后面会被 clamp。
///
/// The position is NaN, or a rotation is not finite. Infinite positions are not invalid;
/// they are clamped later.
pub(crate) fn invalid_position(position: Vec3, (yaw, pitch): (f32, f32)) -> bool {
    position.x.is_nan()
        || position.y.is_nan()
        || position.z.is_nan()
        || !yaw.is_finite()
        || !pitch.is_finite()
}

pub(crate) fn set_rotation(player: &mut Player, yaw: f32, pitch: f32) {
    player.yaw = wrap_degrees(yaw);
    player.pitch = wrap_degrees(pitch).clamp(-MAX_PITCH, MAX_PITCH);
}

/// `use_item` 带来的朝向。官方的 `setYRot`、`setXRot` 分别丢弃不是有限值的角度，不算非法包。
pub(crate) fn snap_rotation(player: &mut Player, yaw: f32, pitch: f32) {
    let yaw = if yaw.is_finite() { yaw } else { player.yaw };
    let pitch = if pitch.is_finite() {
        pitch
    } else {
        player.pitch
    };
    set_rotation(player, yaw, pitch);
}

/// 把角度折到 `[-180, 180)`。全程用 `f32`，和官方的 `Mth.wrapDegrees(float)` 相同。
fn wrap_degrees(angle: f32) -> f32 {
    let mut wrapped = angle % 360.0;
    if wrapped >= 180.0 {
        wrapped -= 360.0;
    }
    if wrapped < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, ChunkPos};
    use mc_bamboo_data::{blocks, packets::play::serverbound};
    use mc_bamboo_protocol::{
        codec::Reader,
        frame::{MAX_FRAME_LEN, find_frame},
        packet::{
            ClientboundPacket,
            play::{ClientInformation, Disconnect, GameMode, PlayRequest},
        },
    };
    use uuid::Uuid;

    use super::*;
    use crate::{inbound::PlayerSink, links::encode_frames};

    const PLAYER: PlayerId = PlayerId(1);
    const SPAWN: Vec3 = Vec3::new(8.5, -60.0, 8.5);

    #[derive(Default)]
    struct SinkState {
        bytes: Vec<u8>,
        closed: bool,
    }

    #[derive(Clone, Default)]
    struct Recorder(std::sync::Arc<std::sync::Mutex<SinkState>>);

    impl PlayerSink for Recorder {
        fn send(&mut self, frames: Vec<u8>) -> bool {
            self.0.lock().unwrap().bytes.extend(frames);
            true
        }

        fn close(&mut self) {
            self.0.lock().unwrap().closed = true;
        }
    }

    impl Recorder {
        fn closed(&self) -> bool {
            self.0.lock().unwrap().closed
        }

        fn bytes(&self) -> Vec<u8> {
            self.0.lock().unwrap().bytes.clone()
        }

        /// 收到的 `player_position` 的 id，依次排列。
        fn teleport_ids(&self) -> Vec<i32> {
            let bytes = self.bytes();
            let mut rest = bytes.as_slice();
            let mut ids = Vec::new();
            while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).unwrap() {
                let end = bounds.header_len + bounds.payload_len;
                let mut reader = Reader::new(&rest[bounds.header_len..end]);
                if reader.varint().unwrap() == PlayerPosition::ID {
                    ids.push(reader.varint().unwrap());
                }
                rest = &rest[end..];
            }
            ids
        }
    }

    /// 一个已经进入世界、完成第一次传送确认的玩家。
    fn settled_region() -> (Region, Recorder) {
        let (mut region, recorder) = awaiting_region();
        on_accept_teleportation(&mut region, PLAYER, 1, SPAWN, (0.0, 0.0));
        (region, recorder)
    }

    /// 一个刚进入世界、还没确认传送的玩家。
    fn awaiting_region() -> (Region, Recorder) {
        let mut region = Region::new(8, 20, false);
        let recorder = Recorder::default();
        region.links.add(PLAYER, Box::new(recorder.clone()));
        let mut player = Player::new(
            PLAYER,
            Uuid::nil(),
            "Kitten".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::default(),
        );
        teleport(
            &mut player,
            &mut region.links,
            &mut region.player_grid,
            region.tick,
            SPAWN,
        );
        region.insert_player(player);
        let joined = region.players.get_mut(&PLAYER).expect("刚放进去");
        joined.motion.sync_goods(SPAWN);
        joined.motion.mark_loaded();
        (region, recorder)
    }

    fn player(region: &Region) -> &Player {
        region.players.get(&PLAYER).expect("玩家在世界里")
    }

    fn advance(region: &mut Region, ticks: u64) {
        for _ in 0..ticks {
            region.advance_tick();
        }
    }

    fn move_to(region: &mut Region, position: Vec3) {
        on_move_player(region, PLAYER, Some(position), None, true, false);
    }

    #[test]
    fn yaw_and_pitch_follow_the_spec_table() {
        for (input, expected) in [
            (190.0, -170.0),
            (-190.0, 170.0),
            (180.0, -180.0),
            (540.0, -180.0),
        ] {
            assert_eq!(wrap_degrees(input), expected, "yaw {input}");
        }

        let (mut region, _) = settled_region();
        for (input, expected) in [(120.0, 90.0), (200.0, -90.0)] {
            on_move_player(&mut region, PLAYER, None, Some((0.0, input)), false, false);
            assert_eq!(player(&region).pitch, expected, "pitch {input}");
        }
    }

    #[test]
    fn high_bits_of_the_move_flags_are_not_an_error() {
        let status_only = serverbound::MOVE_PLAYER_STATUS_ONLY as u8;

        for (flags, on_ground) in [(0xff, true), (0x81, true), (0xfe, false), (0x80, false)] {
            assert_eq!(
                PlayRequest::decode(&[status_only, flags]),
                Ok(PlayRequest::MovePlayer {
                    position: None,
                    rotation: None,
                    on_ground,
                    horizontal_collision: flags & 0x02 != 0,
                }),
                "flags {flags:#x}"
            );
        }
    }

    #[test]
    fn accept_teleportation_frame_is_thirty_five_bytes() {
        let mut frame = vec![0x22, serverbound::ACCEPT_TELEPORTATION as u8, 0x01];
        for coordinate in [SPAWN.x, SPAWN.y, SPAWN.z] {
            frame.extend_from_slice(&coordinate.to_be_bytes());
        }
        frame.extend_from_slice(&[0; 8]);
        assert_eq!(frame.len(), 35);

        let (mut region, _) = awaiting_region();
        let Ok(PlayRequest::AcceptTeleportation {
            id,
            x,
            y,
            z,
            yaw,
            pitch,
        }) = PlayRequest::decode(&frame[1..])
        else {
            panic!("这是规格里的帧");
        };
        on_accept_teleportation(&mut region, PLAYER, id, Vec3::new(x, y, z), (yaw, pitch));

        assert!(!player(&region).teleport.is_awaiting());
        assert_eq!(player(&region).position(), SPAWN);
    }

    #[test]
    fn position_is_clamped_to_the_world_limits() {
        let (mut region, recorder) = settled_region();

        move_to(&mut region, Vec3::new(5.0e7, 9.0e9, -5.0e7));
        assert_eq!(player(&region).position(), SPAWN);
        assert!(player(&region).teleport.is_awaiting());

        on_client_tick_end(&mut region, PLAYER);
        move_to(
            &mut region,
            Vec3::new(f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        assert_eq!(player(&region).position(), SPAWN);
        assert!(!recorder.closed());
    }

    #[test]
    fn nan_position_kicks_with_the_official_reason() {
        for bad in [
            Vec3::new(f64::NAN, 0.0, 0.0),
            Vec3::new(0.0, f64::NAN, 0.0),
            Vec3::new(0.0, 0.0, f64::NAN),
        ] {
            let (mut region, recorder) = settled_region();

            move_to(&mut region, bad);

            let reason = invalid_movement();
            assert!(
                recorder
                    .bytes()
                    .ends_with(&encode_frames(&Disconnect { reason: &reason }))
            );
            assert!(recorder.closed());
            assert_eq!(player(&region).position(), SPAWN);
        }
    }

    #[test]
    fn non_finite_rotation_kicks() {
        for bad in [
            (f32::NAN, 0.0),
            (0.0, f32::NAN),
            (f32::INFINITY, 0.0),
            (0.0, f32::NEG_INFINITY),
        ] {
            let (mut region, recorder) = settled_region();

            on_move_player(&mut region, PLAYER, None, Some(bad), false, false);

            assert!(recorder.closed());
        }
    }

    #[test]
    fn missing_fields_keep_the_current_values() {
        let (mut region, _) = settled_region();
        let target = Vec3::new(9.0, -60.0, 8.5);

        on_move_player(
            &mut region,
            PLAYER,
            Some(target),
            Some((90.0, -45.0)),
            true,
            false,
        );
        assert_eq!(player(&region).position(), target);
        assert_eq!((player(&region).yaw, player(&region).pitch), (90.0, -45.0));
        assert!(player(&region).on_ground);

        on_client_tick_end(&mut region, PLAYER);
        on_move_player(&mut region, PLAYER, None, Some((10.0, 20.0)), false, false);
        assert_eq!(player(&region).position(), target);
        assert_eq!((player(&region).yaw, player(&region).pitch), (10.0, 20.0));
        assert!(!player(&region).on_ground);

        on_move_player(&mut region, PLAYER, Some(SPAWN), None, true, false);
        assert_eq!(player(&region).position(), SPAWN);
        assert_eq!((player(&region).yaw, player(&region).pitch), (10.0, 20.0));

        on_move_player(&mut region, PLAYER, None, None, false, false);
        assert_eq!(player(&region).position(), SPAWN);
        assert_eq!((player(&region).yaw, player(&region).pitch), (10.0, 20.0));
        assert!(!player(&region).on_ground);
    }

    #[test]
    fn walking_up_and_down_stays_in_the_same_chunk() {
        let (mut region, _) = settled_region();

        move_to(&mut region, Vec3::new(8.5, -50.0, 8.5));

        assert_eq!(player(&region).position().y, -50.0);
        assert_eq!(player(&region).chunk_pos(), ChunkPos::new(0, 0));

        on_client_tick_end(&mut region, PLAYER);
        finish_tick(&mut region);
        move_to(&mut region, Vec3::new(-0.5, -50.0, 8.5));
        on_client_tick_end(&mut region, PLAYER);
        finish_tick(&mut region);
        move_to(&mut region, Vec3::new(-0.5, -50.0, 16.5));
        assert_eq!(player(&region).chunk_pos(), ChunkPos::new(-1, 1));
    }

    #[test]
    fn position_waits_for_the_teleport_confirmation() {
        let (mut region, _) = awaiting_region();

        on_move_player(
            &mut region,
            PLAYER,
            Some(Vec3::new(100.0, 0.0, 100.0)),
            Some((190.0, 120.0)),
            true,
            false,
        );

        let waiting = player(&region);
        assert_eq!(waiting.position(), SPAWN);
        assert_eq!((waiting.yaw, waiting.pitch), (-170.0, 90.0));
        assert!(!waiting.on_ground);
        assert!(waiting.teleport.is_awaiting());
    }

    #[test]
    fn nan_is_refused_even_while_waiting_for_a_confirmation() {
        let (mut region, recorder) = awaiting_region();

        move_to(&mut region, Vec3::new(f64::NAN, 0.0, 0.0));

        assert!(recorder.closed());
    }

    #[test]
    fn confirmation_applies_the_position_in_the_packet() {
        let (mut region, _) = awaiting_region();
        let reported = Vec3::new(8.75, -59.5, 8.25);

        on_accept_teleportation(&mut region, PLAYER, 1, reported, (190.0, 200.0));

        let settled = player(&region);
        assert!(!settled.teleport.is_awaiting());
        assert_eq!(settled.position(), reported);
        assert_eq!((settled.yaw, settled.pitch), (-170.0, -90.0));
        assert!(!settled.on_ground);
    }

    #[test]
    fn confirmation_position_is_clamped_too() {
        let (mut region, _) = awaiting_region();

        on_accept_teleportation(
            &mut region,
            PLAYER,
            1,
            Vec3::new(f64::INFINITY, 0.0, 0.0),
            (0.0, 0.0),
        );

        assert_eq!(player(&region).position(), SPAWN);
        assert!(player(&region).teleport.is_awaiting());
    }

    #[test]
    fn confirmation_with_an_old_id_is_ignored() {
        let (mut region, recorder) = awaiting_region();

        on_accept_teleportation(&mut region, PLAYER, 7, SPAWN, (0.0, 0.0));

        assert!(player(&region).teleport.is_awaiting());
        assert!(!recorder.closed());
    }

    #[test]
    fn repeated_confirmation_kicks() {
        let (mut region, recorder) = settled_region();

        on_accept_teleportation(&mut region, PLAYER, 1, SPAWN, (0.0, 0.0));

        assert!(recorder.closed());
    }

    #[test]
    fn confirmation_with_invalid_values_kicks() {
        let (mut region, recorder) = awaiting_region();

        on_accept_teleportation(
            &mut region,
            PLAYER,
            1,
            Vec3::new(f64::NAN, 0.0, 0.0),
            (0.0, 0.0),
        );

        assert!(recorder.closed());
    }

    #[test]
    fn confirmation_with_a_non_finite_rotation_kicks() {
        let (mut region, recorder) = awaiting_region();

        on_accept_teleportation(&mut region, PLAYER, 1, SPAWN, (f32::INFINITY, 0.0));

        assert!(recorder.closed());
    }

    #[test]
    fn teleport_is_resent_with_a_new_id_after_twenty_ticks() {
        let (mut region, recorder) = awaiting_region();

        advance(&mut region, 20);
        move_to(&mut region, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(recorder_ids(&mut region, &recorder), [1]);

        // 同一个客户端 tick 里第二份位置包会被判成非法移动。重发要等下一份客户端 tick。
        //
        // A second position packet in the same client tick is invalid movement. The resend waits
        // for the next client tick.
        on_client_tick_end(&mut region, PLAYER);
        advance(&mut region, 1);
        move_to(&mut region, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(recorder_ids(&mut region, &recorder), [1, 2]);
        assert_eq!(player(&region).position(), SPAWN);

        // 重发之后旧 id 不算数，新 id 才能结束等待。
        on_accept_teleportation(&mut region, PLAYER, 1, SPAWN, (0.0, 0.0));
        assert!(player(&region).teleport.is_awaiting());
        on_accept_teleportation(&mut region, PLAYER, 2, SPAWN, (0.0, 0.0));
        assert!(!player(&region).teleport.is_awaiting());
    }

    #[test]
    fn resend_needs_a_movement_packet() {
        let (mut region, recorder) = awaiting_region();

        advance(&mut region, 100);

        assert_eq!(recorder_ids(&mut region, &recorder), [1]);
    }

    fn recorder_ids(region: &mut Region, recorder: &Recorder) -> Vec<i32> {
        region.links.flush();
        recorder.teleport_ids()
    }

    #[test]
    fn teleport_ids_wrap_before_the_maximum() {
        let mut state = Teleport {
            latest_id: i32::MAX - 2,
            ..Teleport::default()
        };

        assert_eq!(state.next_id(), i32::MAX - 1);
        assert_eq!(state.next_id(), 0);
        assert_eq!(state.next_id(), 1);
    }

    fn player_mut(region: &mut Region) -> &mut Player {
        region.players.get_mut(&PLAYER).expect("玩家在世界里")
    }

    fn set_mode(region: &mut Region, mode: GameMode) {
        player_mut(region).game_mode = mode;
    }

    /// 速度检查允许的上限是 100，等于正好 10 格。再远一格就拉回。
    #[test]
    fn ten_blocks_is_allowed_and_one_more_is_pulled_back() {
        let (mut region, recorder) = settled_region();
        let ten = Vec3::new(SPAWN.x + 10.0, SPAWN.y, SPAWN.z);
        move_to(&mut region, ten);
        assert_eq!(player(&region).position(), ten);
        assert!(!recorder.closed());

        on_client_tick_end(&mut region, PLAYER);
        finish_tick(&mut region);
        move_to(&mut region, Vec3::new(ten.x + 10.0 + 1.0e-6, ten.y, ten.z));
        assert_eq!(player(&region).position(), ten);
        assert!(player(&region).teleport.is_awaiting());
        assert!(!recorder.closed());
    }

    #[test]
    fn a_hundred_block_move_is_pulled_back_without_a_kick() {
        let (mut region, recorder) = settled_region();
        move_to(&mut region, Vec3::new(SPAWN.x + 100.0, SPAWN.y, SPAWN.z));
        assert_eq!(player(&region).position(), SPAWN);
        assert!(player(&region).teleport.is_awaiting());
        assert!(!recorder.closed());
    }

    #[test]
    fn the_sixth_move_packet_in_one_tick_is_judged_as_one_packet() {
        let (mut region, recorder) = settled_region();
        finish_tick(&mut region);
        for step in 1..=5 {
            move_to(
                &mut region,
                Vec3::new(SPAWN.x + f64::from(step) * 0.2, SPAWN.y, SPAWN.z),
            );
            on_client_tick_end(&mut region, PLAYER);
        }
        let far = Vec3::new(SPAWN.x + 15.0, SPAWN.y, SPAWN.z);
        move_to(&mut region, far);
        assert_eq!(player(&region).position().x, SPAWN.x + 1.0);
        assert!(player(&region).teleport.is_awaiting());
        assert!(!recorder.closed());

        let (mut region, _) = settled_region();
        finish_tick(&mut region);
        for step in 1..=4 {
            move_to(
                &mut region,
                Vec3::new(SPAWN.x + f64::from(step) * 0.2, SPAWN.y, SPAWN.z),
            );
            on_client_tick_end(&mut region, PLAYER);
        }
        move_to(&mut region, far);
        assert_eq!(player(&region).position(), far);
    }

    #[test]
    fn walking_into_stone_is_pulled_back_for_survival_and_creative() {
        for mode in [GameMode::Survival, GameMode::Creative, GameMode::Adventure] {
            let (mut region, recorder) = settled_region();
            set_mode(&mut region, mode);
            region
                .chunks
                .set_block(BlockPos::new(9, -60, 8), blocks::STONE);
            move_to(&mut region, Vec3::new(9.2, SPAWN.y, SPAWN.z));
            assert_eq!(player(&region).position(), SPAWN, "{mode:?}");
            assert!(player(&region).teleport.is_awaiting(), "{mode:?}");
            assert!(!recorder.closed(), "{mode:?}");
        }
    }

    #[test]
    fn a_player_already_inside_a_block_may_move_within_it() {
        let (mut region, _) = settled_region();
        set_mode(&mut region, GameMode::Survival);
        region
            .chunks
            .set_block(BlockPos::new(8, -60, 8), blocks::STONE);
        let inside = Vec3::new(8.6, SPAWN.y, SPAWN.z);
        move_to(&mut region, inside);
        assert_eq!(player(&region).position(), inside);
        assert!(!player(&region).teleport.is_awaiting());
    }

    #[test]
    fn a_spectator_passes_through_stone_only_after_the_tick_sets_no_physics() {
        let (mut region, _) = settled_region();
        set_mode(&mut region, GameMode::Spectator);
        region
            .chunks
            .set_block(BlockPos::new(9, -60, 8), blocks::STONE);
        let into = Vec3::new(9.2, SPAWN.y, SPAWN.z);
        move_to(&mut region, into);
        assert_eq!(player(&region).position(), SPAWN);
        assert!(player(&region).teleport.is_awaiting());

        on_client_tick_end(&mut region, PLAYER);
        let teleport_id = player(&region).teleport.latest_id;
        on_accept_teleportation(&mut region, PLAYER, teleport_id, SPAWN, (0.0, 0.0));
        finish_tick(&mut region);
        move_to(&mut region, into);
        assert_eq!(player(&region).position(), into);
    }

    #[test]
    fn floating_for_81_ticks_kicks_survival_and_adventure_only() {
        for mode in [GameMode::Survival, GameMode::Adventure] {
            let (mut region, recorder) = settled_region();
            set_mode(&mut region, mode);
            crate::player_move::place_player(&mut region, PLAYER, Vec3::new(8.5, 80.0, 8.5));
            move_to(&mut region, Vec3::new(8.5, 80.0, 8.5));
            assert!(player(&region).motion.client_is_floating, "{mode:?}");
            for _ in 0..80 {
                finish_tick(&mut region);
            }
            assert!(!recorder.closed(), "{mode:?}");
            finish_tick(&mut region);
            assert!(recorder.closed(), "{mode:?}");
        }

        for mode in [GameMode::Creative, GameMode::Spectator] {
            let (mut region, recorder) = settled_region();
            set_mode(&mut region, mode);
            crate::player_move::place_player(&mut region, PLAYER, Vec3::new(8.5, 80.0, 8.5));
            move_to(&mut region, Vec3::new(8.5, 80.0, 8.5));
            assert!(!player(&region).motion.client_is_floating, "{mode:?}");
            for _ in 0..81 {
                finish_tick(&mut region);
            }
            assert!(!recorder.closed(), "{mode:?}");
        }
    }

    #[test]
    fn movement_before_the_client_has_loaded_is_ignored() {
        let mut region = Region::new(8, 20, false);
        let recorder = Recorder::default();
        region.links.add(PLAYER, Box::new(recorder.clone()));
        let mut joined = Player::new(
            PLAYER,
            Uuid::nil(),
            "Kitten".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            SPAWN,
        );
        joined.motion.sync_goods(SPAWN);
        region.insert_player(joined);

        move_to(&mut region, Vec3::new(9.0, SPAWN.y, SPAWN.z));
        assert_eq!(player(&region).position(), SPAWN);
        assert!(!recorder.closed());
    }

    #[test]
    fn two_position_packets_in_one_client_tick_kick() {
        let (mut region, recorder) = settled_region();
        move_to(&mut region, Vec3::new(9.0, SPAWN.y, SPAWN.z));
        move_to(&mut region, Vec3::new(10.0, SPAWN.y, SPAWN.z));
        assert!(recorder.closed());
    }

    #[test]
    fn a_client_tick_end_allows_another_position() {
        let (mut region, recorder) = settled_region();
        let first = Vec3::new(9.0, SPAWN.y, SPAWN.z);
        move_to(&mut region, first);
        on_client_tick_end(&mut region, PLAYER);
        finish_tick(&mut region);
        let second = Vec3::new(10.0, SPAWN.y, SPAWN.z);
        move_to(&mut region, second);
        assert_eq!(player(&region).position(), second);
        assert!(!recorder.closed());
    }

    #[test]
    fn fall_distance_accumulates_and_resets_on_ground_without_damage() {
        let (mut region, _) = settled_region();
        crate::player_move::place_player(&mut region, PLAYER, Vec3::new(8.5, 80.0, 8.5));
        on_move_player(
            &mut region,
            PLAYER,
            Some(Vec3::new(8.5, 78.0, 8.5)),
            None,
            false,
            false,
        );
        let fell = player(&region).motion.fall_distance;
        // 服务端玩家不算 `move` 里面那次摔落，只记接受之后的 `doCheckFallDamage`，所以是 2 不是 4。
        //
        // A server player skips the fall record inside `move` and keeps only `doCheckFallDamage`
        // after the move is accepted, so this is 2 rather than 4.
        assert!((fell - 2.0).abs() < 1.0e-6, "{fell}");

        on_client_tick_end(&mut region, PLAYER);
        finish_tick(&mut region);
        move_to(&mut region, Vec3::new(8.5, 78.0, 8.5));
        assert_eq!(player(&region).motion.fall_distance, 0.0);
        assert!(player(&region).on_ground);
    }

    #[test]
    fn a_legal_position_past_the_horizontal_limit_is_clamped_and_kept() {
        let (mut region, recorder) = settled_region();
        let near = Vec3::new(29_999_995.0, SPAWN.y, SPAWN.z);
        crate::player_move::place_player(&mut region, PLAYER, near);
        finish_tick(&mut region);
        move_to(&mut region, Vec3::new(30_000_010.0, SPAWN.y, SPAWN.z));
        assert_eq!(player(&region).position().x, 30_000_000.0);
        assert!(!player(&region).teleport.is_awaiting());
        assert!(!recorder.closed());
    }

    #[test]
    fn the_horizontal_collision_flag_is_stored() {
        let (mut region, _) = settled_region();
        on_move_player(
            &mut region,
            PLAYER,
            Some(Vec3::new(8.7, SPAWN.y, SPAWN.z)),
            None,
            true,
            true,
        );
        assert!(player(&region).motion.client_horizontal_collision);
        assert!(player(&region).on_ground);
    }
}
