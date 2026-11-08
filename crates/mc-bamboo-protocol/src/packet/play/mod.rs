//! Play 状态：玩家已经在世界里。
//!
//! 按主题分成几个子模块，所有公开项都从这里导出。

mod block;
mod chat;
mod entity;
mod menu;
mod movement;
mod player_info;
mod request;
mod sound;

pub use block::*;
pub use chat::*;
pub use entity::*;
pub use menu::*;
pub use movement::*;
pub use player_info::*;
pub use request::*;
pub use sound::*;

use mc_bamboo_data::packets::play::clientbound;

use super::ClientboundPacket;
use crate::{
    codec::{write_bool, write_i32, write_i64, write_string, write_varint, write_varlong},
    text::Component,
};

/// 游戏模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

/// 进入世界时发送的第一个数据包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Login<'a> {
    pub entity_id: i32,
    pub dimensions: &'a [&'a str],
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub dimension_type: i32,
    pub dimension: &'a str,
    pub hashed_seed: i64,
    pub game_mode: GameMode,
    pub is_flat: bool,
    pub sea_level: i32,
    pub online_mode: bool,
}

impl ClientboundPacket for Login<'_> {
    const ID: i32 = clientbound::LOGIN;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.entity_id.to_be_bytes());
        // 极限模式。
        write_bool(out, false);
        write_varint(out, self.dimensions.len() as i32);
        for dimension in self.dimensions {
            write_string(out, dimension);
        }
        write_varint(out, self.max_players);
        write_varint(out, self.view_distance);
        write_varint(out, self.simulation_distance);
        // 依次是：隐藏调试信息、显示死亡画面、限制合成。
        write_bool(out, false);
        write_bool(out, true);
        write_bool(out, false);

        write_varint(out, self.dimension_type);
        write_string(out, self.dimension);
        out.extend_from_slice(&self.hashed_seed.to_be_bytes());
        write_varint(out, self.game_mode as i32);
        // 上一个游戏模式：可选值存成“值加一”，0 表示没有。
        write_varint(out, 0);
        // 调试世界。
        write_bool(out, false);
        write_bool(out, self.is_flat);
        // 上次死亡的位置。
        write_bool(out, false);
        // 传送门冷却。
        write_varint(out, 0);
        write_varint(out, self.sea_level);

        write_bool(out, self.online_mode);
        // 强制聊天签名。
        write_bool(out, false);
    }
}

/// 玩家能力，创造模式靠它才能飞行。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerAbilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub can_fly: bool,
    pub instant_build: bool,
    pub flying_speed: f32,
    pub walking_speed: f32,
}

impl ClientboundPacket for PlayerAbilities {
    const ID: i32 = clientbound::PLAYER_ABILITIES;

    fn write_body(&self, out: &mut Vec<u8>) {
        let flags = u8::from(self.invulnerable)
            | u8::from(self.flying) << 1
            | u8::from(self.can_fly) << 2
            | u8::from(self.instant_build) << 3;
        out.push(flags);
        out.extend_from_slice(&self.flying_speed.to_be_bytes());
        out.extend_from_slice(&self.walking_speed.to_be_bytes());
    }
}

/// 把玩家传送到一个绝对位置，客户端要用 [`PlayRequest::AcceptTeleportation`] 确认。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPosition {
    pub teleport_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl ClientboundPacket for PlayerPosition {
    const ID: i32 = clientbound::PLAYER_POSITION;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.teleport_id);
        for coordinate in [self.x, self.y, self.z] {
            out.extend_from_slice(&coordinate.to_be_bytes());
        }
        // 速度。
        out.extend_from_slice(&[0; 24]);
        out.extend_from_slice(&self.yaw.to_be_bytes());
        out.extend_from_slice(&self.pitch.to_be_bytes());
        // 哪些字段是相对值的位掩码，这里全部是绝对值。
        out.extend_from_slice(&0i32.to_be_bytes());
    }
}

/// 同步时间。26.x 起每个世界时钟单独同步。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetTime {
    pub game_time: i64,
    pub clock: i32,
    pub clock_ticks: i64,
    /// 时钟的流速，0 表示时间静止。
    pub clock_rate: f32,
}

impl ClientboundPacket for SetTime {
    const ID: i32 = clientbound::SET_TIME;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.game_time.to_be_bytes());
        write_varint(out, 1);
        write_varint(out, self.clock);
        write_varlong(out, self.clock_ticks);
        // 不足一刻的部分。
        out.extend_from_slice(&0f32.to_be_bytes());
        out.extend_from_slice(&self.clock_rate.to_be_bytes());
    }
}

/// 通知客户端开始等待区块，客户端收到后才会结束加载画面的流程。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelChunksLoadStart;

impl ClientboundPacket for LevelChunksLoadStart {
    const ID: i32 = clientbound::GAME_EVENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        const LEVEL_CHUNKS_LOAD_START: u8 = 13;
        out.push(LEVEL_CHUNKS_LOAD_START);
        out.extend_from_slice(&0f32.to_be_bytes());
    }
}

/// 告诉客户端以哪个区块为中心加载。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetChunkCacheCenter {
    pub x: i32,
    pub z: i32,
}

impl ClientboundPacket for SetChunkCacheCenter {
    const ID: i32 = clientbound::SET_CHUNK_CACHE_CENTER;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.x);
        write_varint(out, self.z);
    }
}

/// 让客户端卸载一个区块。这个包不走批次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForgetLevelChunk {
    pub x: i32,
    pub z: i32,
}

impl ClientboundPacket for ForgetLevelChunk {
    const ID: i32 = clientbound::FORGET_LEVEL_CHUNK;

    fn write_body(&self, out: &mut Vec<u8>) {
        // 线上是一个 Long，低 32 位是 x，所以大端写出来 z 在前。
        write_i32(out, self.z);
        write_i32(out, self.x);
    }
}

/// 一批区块开始。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkBatchStart;

impl ClientboundPacket for ChunkBatchStart {
    const ID: i32 = clientbound::CHUNK_BATCH_START;

    fn write_body(&self, _out: &mut Vec<u8>) {}
}

/// 一批区块结束，客户端据此计算合适的发送速度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkBatchFinished {
    pub batch_size: i32,
}

impl ClientboundPacket for ChunkBatchFinished {
    const ID: i32 = clientbound::CHUNK_BATCH_FINISHED;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.batch_size);
    }
}

/// 一个区块。`body` 是 [`super::chunk::encode_chunk_body`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelChunkWithLight<'a> {
    pub x: i32,
    pub z: i32,
    pub body: &'a [u8],
}

impl ClientboundPacket for LevelChunkWithLight<'_> {
    const ID: i32 = clientbound::LEVEL_CHUNK_WITH_LIGHT;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.x.to_be_bytes());
        out.extend_from_slice(&self.z.to_be_bytes());
        out.extend_from_slice(self.body);
    }
}

/// 心跳。客户端要在时限内原样发回这个数字，否则会被断开。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAlive {
    pub id: i64,
}

impl ClientboundPacket for KeepAlive {
    const ID: i32 = clientbound::KEEP_ALIVE;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.id.to_be_bytes());
    }
}

/// 对 [`PlayRequest::PingRequest`] 的回应，`time` 原样返回。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PongResponse {
    pub time: i64,
}

impl ClientboundPacket for PongResponse {
    const ID: i32 = clientbound::PONG_RESPONSE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_i64(out, self.time);
    }
}

/// 在 Play 状态断开连接。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Disconnect<'a> {
    pub reason: &'a Component,
}

impl ClientboundPacket for Disconnect<'_> {
    const ID: i32 = clientbound::DISCONNECT;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.reason.write(out);
    }
}

/// 一组数据包的分隔符。两个分隔符之间的包在客户端主线程的同一个任务里处理。
///
/// 漏掉第二个分隔符时，后面的包全部被吞掉，所以要把两个分隔符和内容一次放进发送队列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleDelimiter;

impl ClientboundPacket for BundleDelimiter {
    const ID: i32 = clientbound::BUNDLE_DELIMITER;

    fn write_body(&self, _out: &mut Vec<u8>) {}
}

/// 统计数据。客户端只是在等回应好结束统计界面的加载状态，所以总是发空列表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AwardStats;

impl ClientboundPacket for AwardStats {
    const ID: i32 = clientbound::AWARD_STATS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, frame, hex};

    #[test]
    fn packet_ids_match_the_specs() {
        let ids = [
            (BundleDelimiter::ID, 0),
            (AddEntity::ID, 1),
            (AwardStats::ID, 3),
            (BlockChangedAck::ID, 4),
            (BlockEntityData::ID, 6),
            (BlockEvent::ID, 7),
            (BlockUpdate::ID, 8),
            (ContainerClose::ID, 17),
            (ContainerSetContent::ID, 18),
            (ContainerSetSlot::ID, 20),
            (DisguisedChat::ID, 33),
            (EntityPositionSync::ID, 35),
            (ForgetLevelChunk::ID, 38),
            (LevelEvent::ID, 47),
            (MoveEntityPos::ID, 54),
            (MoveEntityPosRot::ID, 55),
            (MoveEntityRot::ID, 57),
            (OpenScreen::ID, 60),
            (OpenSignEditor::ID, 61),
            (PongResponse::ID, 63),
            (PlayerInfoRemove::ID, 70),
            (PlayerInfoUpdate::ID, 71),
            (RemoveEntities::ID, 78),
            (RotateHead::ID, 85),
            (SectionBlocksUpdate::ID, 86),
            (SetCursorItem::ID, 98),
            (SetEntityData::ID, 101),
            (SetEquipment::ID, 104),
            (SetHeldSlot::ID, 107),
            (SwingAnimation::ID, 123),
            (SystemChat::ID, 124),
            (UpdateAttributes::ID, 134),
        ];
        for (actual, expected) in ids {
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn packs_ability_flags() {
        let abilities = PlayerAbilities {
            invulnerable: true,
            flying: false,
            can_fly: true,
            instant_build: true,
            flying_speed: 0.05,
            walking_speed: 0.1,
        };
        let mut body = Vec::new();
        abilities.write_body(&mut body);
        assert_eq!(body[0], 0b1101);
        assert_eq!(body.len(), 9);
    }

    #[test]
    fn position_has_a_fixed_layout() {
        let position = PlayerPosition {
            teleport_id: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
        };
        let mut body = Vec::new();
        position.write_body(&mut body);
        // ID、三个坐标、三个速度、两个角度、相对标志。
        assert_eq!(body.len(), 1 + 24 + 24 + 8 + 4);
    }

    #[test]
    fn position_frame_matches_official_bytes() {
        let position = PlayerPosition {
            teleport_id: 1,
            x: 8.5,
            y: -60.0,
            z: 8.5,
            yaw: 0.0,
            pitch: 0.0,
        };
        assert_eq!(
            frame(&position),
            hex("3e 49 01
                 40 21 00 00 00 00 00 00  c0 4e 00 00 00 00 00 00  40 21 00 00 00 00 00 00
                 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00
                 00 00 00 00  00 00 00 00
                 00 00 00 00")
        );
    }

    #[test]
    fn level_chunks_load_start_matches_official_bytes() {
        assert_eq!(frame(&LevelChunksLoadStart), hex("06 27 0d 00 00 00 00"));
    }

    #[test]
    fn disconnect_reason_is_an_nbt_string() {
        let reason = Component::text("hi");
        assert_eq!(body(&Disconnect { reason: &reason }), [8, 0, 2, b'h', b'i']);
    }

    #[test]
    fn set_chunk_cache_center_matches_official_bytes() {
        assert_eq!(
            frame(&SetChunkCacheCenter { x: 1, z: 0 }),
            hex("03 60 01 00")
        );
        assert_eq!(
            frame(&SetChunkCacheCenter { x: -1, z: 300 }),
            hex("08 60 ff ff ff ff 0f ac 02")
        );
    }

    #[test]
    fn forget_level_chunk_writes_z_first() {
        assert_eq!(
            frame(&ForgetLevelChunk { x: -3, z: 5 }),
            hex("09 26 00 00 00 05 ff ff ff fd")
        );
        assert_eq!(
            frame(&ForgetLevelChunk { x: -3, z: -3 }),
            hex("09 26 ff ff ff fd ff ff ff fd")
        );
    }

    #[test]
    fn chunk_batch_frames_match_official_bytes() {
        assert_eq!(frame(&ChunkBatchStart), hex("01 0c"));
        assert_eq!(
            frame(&ChunkBatchFinished { batch_size: 9 }),
            hex("02 0b 09")
        );
        assert_eq!(
            frame(&ChunkBatchFinished { batch_size: 300 }),
            hex("03 0b ac 02")
        );
    }

    #[test]
    fn pong_response_echoes_the_time() {
        let packet = PongResponse {
            time: 0x0000_018b_cfe5_6800,
        };
        assert_eq!(body(&packet), hex("00 00 01 8b cf e5 68 00"));
    }

    #[test]
    fn bundle_delimiter_is_an_empty_packet() {
        assert_eq!(frame(&BundleDelimiter), hex("01 00"));
    }

    #[test]
    fn award_stats_is_an_empty_list() {
        assert_eq!(body(&AwardStats), hex("00"));
    }
}
