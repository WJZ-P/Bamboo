//! Login 状态：确认玩家身份。

use mc_bamboo_data::packets::login::{clientbound, serverbound};
use uuid::Uuid;

use super::ClientboundPacket;
use crate::codec::{DecodeError, Reader, write_string, write_uuid, write_varint};

/// 玩家名的最大长度。
pub const MAX_PLAYER_NAME_CHARS: usize = 16;

/// 客户端在 Login 状态发送的数据包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginRequest<'a> {
    /// 登录请求，带着玩家名。
    Hello { name: &'a str, uuid: Uuid },
    /// 客户端已经收到登录成功，准备进入 Configuration 状态。
    Acknowledged,
}

impl<'a> LoginRequest<'a> {
    /// 解码一帧的负载，负载以数据包 ID 开头。
    pub fn decode(payload: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        let request = match reader.varint()? {
            serverbound::HELLO => Self::Hello {
                name: reader.string(MAX_PLAYER_NAME_CHARS)?,
                uuid: reader.uuid()?,
            },
            serverbound::LOGIN_ACKNOWLEDGED => Self::Acknowledged,
            unknown => return Err(DecodeError::UnknownPacket(unknown)),
        };
        reader.finish()?;
        Ok(request)
    }
}

/// 拒绝登录。原因是 JSON 形式的文本组件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginDisconnect<'a> {
    pub reason_json: &'a str,
}

impl ClientboundPacket for LoginDisconnect<'_> {
    const ID: i32 = clientbound::LOGIN_DISCONNECT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.reason_json);
    }
}

/// 告诉客户端从下一个包开始启用压缩，以及压缩的阈值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginCompression {
    pub threshold: i32,
}

impl ClientboundPacket for LoginCompression {
    const ID: i32 = clientbound::LOGIN_COMPRESSION;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.threshold);
    }
}

/// 登录成功。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginFinished<'a> {
    pub uuid: Uuid,
    pub name: &'a str,
    /// 每次服务端启动时生成的会话 ID，从 26.3 开始才有这个字段。
    pub session_id: Uuid,
}

impl ClientboundPacket for LoginFinished<'_> {
    const ID: i32 = clientbound::LOGIN_FINISHED;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_uuid(out, self.uuid);
        write_string(out, self.name);
        // 皮肤等属性的数量。离线模式没有属性。
        write_varint(out, 0);
        write_uuid(out, self.session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::append_frame;

    #[test]
    fn decodes_hello() {
        let uuid = Uuid::from_u128(1);
        let mut payload = vec![serverbound::HELLO as u8];
        write_string(&mut payload, "Kitten");
        write_uuid(&mut payload, uuid);

        assert_eq!(
            LoginRequest::decode(&payload),
            Ok(LoginRequest::Hello {
                name: "Kitten",
                uuid
            })
        );
    }

    #[test]
    fn rejects_a_name_over_the_limit() {
        let mut payload = vec![serverbound::HELLO as u8];
        write_string(&mut payload, &"a".repeat(MAX_PLAYER_NAME_CHARS + 1));
        write_uuid(&mut payload, Uuid::nil());

        assert_eq!(
            LoginRequest::decode(&payload),
            Err(DecodeError::StringTooLong {
                max_chars: MAX_PLAYER_NAME_CHARS
            })
        );
    }

    #[test]
    fn decodes_acknowledged() {
        assert_eq!(
            LoginRequest::decode(&[serverbound::LOGIN_ACKNOWLEDGED as u8]),
            Ok(LoginRequest::Acknowledged)
        );
    }

    #[test]
    fn encodes_login_disconnect() {
        let mut out = Vec::new();
        append_frame(&mut out, &LoginDisconnect { reason_json: "{}" }).unwrap();
        assert_eq!(out, [0x04, 0x00, 0x02, b'{', b'}']);
    }

    #[test]
    fn encodes_login_compression() {
        let mut out = Vec::new();
        append_frame(&mut out, &LoginCompression { threshold: 256 }).unwrap();
        // 帧长 3：包 ID 0x03，阈值 256 的 VarInt 是 0x80 0x02。
        assert_eq!(out, [0x03, 0x03, 0x80, 0x02]);
    }

    #[test]
    fn login_finished_ends_with_the_session_id() {
        let session_id = Uuid::from_u128(0xabcd);
        let packet = LoginFinished {
            uuid: Uuid::from_u128(1),
            name: "Kitten",
            session_id,
        };
        let mut body = Vec::new();
        packet.write_body(&mut body);

        assert_eq!(&body[body.len() - 16..], session_id.as_bytes());
        assert_eq!(body[body.len() - 17], 0, "properties list should be empty");
    }
}
