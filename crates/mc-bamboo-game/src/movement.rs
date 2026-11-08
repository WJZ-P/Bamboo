//! 玩家移动和传送确认。
//!
//! 客户端说了算：M2 没有服务端物理，所以这里只挡非法值，再把位置记下来。
//! 区块由 tick 的跟踪阶段根据新位置处理，实体同步也是每 tick 比较位置。

use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::{packet::play::PlayerPosition, text::Component};

use crate::{
    inbound::PlayerId,
    links::Links,
    player::Player,
    player_grid::PlayerGrid,
    region::{Region, RegionTick},
};

const MAX_HORIZONTAL: f64 = 3.0e7;
const MAX_VERTICAL: f64 = 2.0e7;
const MAX_PITCH: f32 = 90.0;
/// 一次传送多久没被确认就重发。只有收到移动包时才检查，不是定时器。
const TELEPORT_RESEND_TICKS: u64 = 20;

const INVALID_MOVEMENT: &str = "multiplayer.disconnect.invalid_player_movement";

/// 传送确认的状态。
///
/// 服务端传送之后，网络上可能还有客户端在传送之前发出的移动包。
/// 确认到达之前丢掉它们的位置，否则玩家会被拉回原处。
#[derive(Debug, Default)]
pub(crate) struct Teleport {
    /// 最新一次传送的 id。第一次是 1，和 `player_position` 里的相同。
    latest_id: i32,
    /// 有传送没被确认时，是传送之后的服务端位置。
    awaiting_position: Option<Vec3>,
    /// 最新一次传送发出的时刻。
    sent_at: RegionTick,
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
    };
    links.send(player.id, &packet);
}

/// 处理四种移动包。线上没有的字段是 `None`，保持玩家当前的值。
pub(crate) fn on_move_player(
    region: &mut Region,
    id: PlayerId,
    position: Option<Vec3>,
    rotation: Option<(f32, f32)>,
    on_ground: bool,
) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if has_invalid_values(position.unwrap_or_default(), rotation.unwrap_or_default()) {
        region.links.kick(id, &invalid_movement());
        return;
    }

    if let Some(awaiting) = player.teleport.awaiting_position {
        // 这个包可能是传送之前发的，位置不能信。朝向不影响别的东西，可以照常更新。
        if region.tick.since(player.teleport.sent_at) > TELEPORT_RESEND_TICKS {
            teleport(
                player,
                &mut region.links,
                &mut region.player_grid,
                region.tick,
                awaiting,
            );
        }
        if let Some((yaw, pitch)) = rotation {
            set_rotation(player, yaw, pitch);
        }
        return;
    }

    let position = position.unwrap_or(player.position());
    let (yaw, pitch) = rotation.unwrap_or((player.yaw, player.pitch));
    accept_movement(
        player,
        &mut region.player_grid,
        position,
        yaw,
        pitch,
        on_ground,
    );
}

/// 处理 `accept_teleportation`。
///
/// 客户端确认之后不再发移动包，所以确认包里的位置就是它的下一次移动。
pub(crate) fn on_accept_teleportation(
    region: &mut Region,
    id: PlayerId,
    teleport_id: i32,
    position: Vec3,
    (yaw, pitch): (f32, f32),
) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    // 重发之后旧 id 的确认走这里，位置继续被挡住。
    if teleport_id != player.teleport.latest_id {
        return;
    }
    let Some(awaiting) = player.teleport.awaiting_position.take() else {
        region.links.kick(id, &invalid_movement());
        return;
    };
    if has_invalid_values(position, (yaw, pitch)) {
        region.links.kick(id, &invalid_movement());
        return;
    }

    player.set_position(&mut region.player_grid, awaiting);
    accept_movement(player, &mut region.player_grid, position, yaw, pitch, false);
}

fn invalid_movement() -> Component {
    Component::translate(INVALID_MOVEMENT, [])
}

/// 位置是 NaN，或者朝向不是有限值。±无穷的位置不算非法，后面会被 clamp。
fn has_invalid_values(position: Vec3, (yaw, pitch): (f32, f32)) -> bool {
    position.x.is_nan()
        || position.y.is_nan()
        || position.z.is_nan()
        || !yaw.is_finite()
        || !pitch.is_finite()
}

fn accept_movement(
    player: &mut Player,
    grid: &mut PlayerGrid,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) {
    let position = Vec3::new(
        position.x.clamp(-MAX_HORIZONTAL, MAX_HORIZONTAL),
        position.y.clamp(-MAX_VERTICAL, MAX_VERTICAL),
        position.z.clamp(-MAX_HORIZONTAL, MAX_HORIZONTAL),
    );
    player.set_position(grid, position);
    set_rotation(player, yaw, pitch);
    player.on_ground = on_ground;
}

fn set_rotation(player: &mut Player, yaw: f32, pitch: f32) {
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
    use mc_bamboo_core::ChunkPos;
    use mc_bamboo_data::packets::play::serverbound;
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
        on_move_player(region, PLAYER, Some(position), None, true);
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
            on_move_player(&mut region, PLAYER, None, Some((0.0, input)), false);
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
        assert_eq!(player(&region).position(), Vec3::new(3.0e7, 2.0e7, -3.0e7));

        move_to(
            &mut region,
            Vec3::new(f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        assert_eq!(player(&region).position(), Vec3::new(3.0e7, -2.0e7, -3.0e7));
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

            on_move_player(&mut region, PLAYER, None, Some(bad), false);

            assert!(recorder.closed());
        }
    }

    #[test]
    fn missing_fields_keep_the_current_values() {
        let (mut region, _) = settled_region();
        let target = Vec3::new(1.0, 2.0, 3.0);

        on_move_player(&mut region, PLAYER, Some(target), Some((90.0, -45.0)), true);
        assert_eq!(player(&region).position(), target);
        assert_eq!((player(&region).yaw, player(&region).pitch), (90.0, -45.0));
        assert!(player(&region).on_ground);

        on_move_player(&mut region, PLAYER, None, Some((10.0, 20.0)), false);
        assert_eq!(player(&region).position(), target);
        assert_eq!((player(&region).yaw, player(&region).pitch), (10.0, 20.0));
        assert!(!player(&region).on_ground);

        on_move_player(&mut region, PLAYER, Some(SPAWN), None, true);
        assert_eq!(player(&region).position(), SPAWN);
        assert_eq!((player(&region).yaw, player(&region).pitch), (10.0, 20.0));

        on_move_player(&mut region, PLAYER, None, None, false);
        assert_eq!(player(&region).position(), SPAWN);
        assert_eq!((player(&region).yaw, player(&region).pitch), (10.0, 20.0));
        assert!(!player(&region).on_ground);
    }

    #[test]
    fn walking_up_and_down_stays_in_the_same_chunk() {
        let (mut region, _) = settled_region();

        move_to(&mut region, Vec3::new(8.5, 300.0, 8.5));

        assert_eq!(player(&region).position().y, 300.0);
        assert_eq!(player(&region).chunk_pos(), ChunkPos::new(0, 0));

        move_to(&mut region, Vec3::new(-0.5, 300.0, 16.5));
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

        assert_eq!(player(&region).position(), Vec3::new(3.0e7, 0.0, 0.0));
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
}
