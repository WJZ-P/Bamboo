//! Play 状态：玩家已经在世界里。

use mc_bamboo_data::packets::play::{clientbound, serverbound};
use uuid::Uuid;

use super::{ClientboundPacket, write_text_component};
use crate::codec::{
    DecodeError, Reader, write_bool, write_string, write_uuid, write_varint, write_varlong,
};

/// 客户端在 Play 状态发送的数据包。
///
/// 这里只解码服务端关心的包。其他合法的包归入 `Other`，由调用方忽略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayRequest {
    /// 对 [`KeepAlive`] 的回应。
    KeepAlive(i64),
    /// 客户端确认了 [`PlayerPosition`] 这次传送。
    AcceptTeleportation(i32),
    Other(i32),
}

impl PlayRequest {
    /// 解码一帧的负载，负载以数据包 ID 开头。
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        match reader.varint()? {
            serverbound::KEEP_ALIVE => {
                let id = reader.i64()?;
                reader.finish()?;
                Ok(Self::KeepAlive(id))
            }
            serverbound::ACCEPT_TELEPORTATION => Ok(Self::AcceptTeleportation(reader.varint()?)),
            other => Ok(Self::Other(other)),
        }
    }
}

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

/// 把一名玩家加入 Tab 列表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddPlayerInfo<'a> {
    pub uuid: Uuid,
    pub name: &'a str,
    pub game_mode: GameMode,
}

impl ClientboundPacket for AddPlayerInfo<'_> {
    const ID: i32 = clientbound::PLAYER_INFO_UPDATE;

    fn write_body(&self, out: &mut Vec<u8>) {
        // 动作位掩码，依次是 ADD_PLAYER、UPDATE_GAME_MODE、UPDATE_LISTED。
        const ACTIONS: u8 = 1 << 0 | 1 << 2 | 1 << 3;
        out.push(ACTIONS);
        write_varint(out, 1);
        write_uuid(out, self.uuid);
        write_string(out, self.name);
        // 皮肤等属性的数量。
        write_varint(out, 0);
        write_varint(out, self.game_mode as i32);
        write_bool(out, true);
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

/// 在 Play 状态断开连接。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disconnect<'a> {
    pub reason: &'a str,
}

impl ClientboundPacket for Disconnect<'_> {
    const ID: i32 = clientbound::DISCONNECT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_text_component(out, self.reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_keep_alive_and_teleport_confirmation() {
        let mut keep_alive = vec![serverbound::KEEP_ALIVE as u8];
        keep_alive.extend_from_slice(&42i64.to_be_bytes());
        assert_eq!(
            PlayRequest::decode(&keep_alive),
            Ok(PlayRequest::KeepAlive(42))
        );

        let accept = [serverbound::ACCEPT_TELEPORTATION as u8, 7];
        assert_eq!(
            PlayRequest::decode(&accept),
            Ok(PlayRequest::AcceptTeleportation(7))
        );
    }

    #[test]
    fn leaves_unhandled_packets_to_the_caller() {
        assert_eq!(
            PlayRequest::decode(&[serverbound::CLIENT_TICK_END as u8]),
            Ok(PlayRequest::Other(serverbound::CLIENT_TICK_END))
        );
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
    fn disconnect_reason_is_an_nbt_string() {
        let mut body = Vec::new();
        Disconnect { reason: "hi" }.write_body(&mut body);
        assert_eq!(body, [8, 0, 2, b'h', b'i']);
    }
}
