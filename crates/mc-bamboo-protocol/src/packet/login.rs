//! Login 状态：确认玩家身份。

use mc_bamboo_core::profile::Property;
use mc_bamboo_data::packets::login::{clientbound, serverbound};
use uuid::Uuid;

use super::ClientboundPacket;
use crate::codec::{
    DecodeError, Reader, write_bool, write_byte_array, write_properties, write_string, write_uuid,
    write_varint,
};

/// 玩家名的最大长度。
pub const MAX_PLAYER_NAME_CHARS: usize = 16;

/// 用服务端公钥加密出来的那两段数据的长度上限。
///
/// 1024 位 RSA 的输出正好是 128 字节，留一点余量，太长的直接当畸形包。
const MAX_ENCRYPTED_LEN: usize = 256;

/// 客户端在 Login 状态发送的数据包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginRequest<'a> {
    /// 登录请求，带着玩家名。
    Hello { name: &'a str, uuid: Uuid },
    /// 客户端用服务端公钥加密的 AES 密钥和 challenge。
    Key {
        shared_secret: &'a [u8],
        challenge: &'a [u8],
    },
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
            serverbound::KEY => Self::Key {
                shared_secret: reader.byte_array(MAX_ENCRYPTED_LEN)?,
                challenge: reader.byte_array(MAX_ENCRYPTED_LEN)?,
            },
            serverbound::LOGIN_ACKNOWLEDGED => Self::Acknowledged,
            unknown => return Err(DecodeError::UnknownPacket(unknown)),
        };
        reader.finish()?;
        Ok(request)
    }
}

/// 要求客户端加密这条连接：公布服务端公钥，并要求把 challenge 原样加密送回来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginHello<'a> {
    /// 官方这里一直是空串。
    pub server_id: &'a str,
    /// 公钥的 X.509 编码。
    pub public_key: &'a [u8],
    pub challenge: &'a [u8],
    /// 服务端要不要去问 Mojang。关掉正版验证但仍然加密时它是 false。
    pub should_authenticate: bool,
}

impl ClientboundPacket for LoginHello<'_> {
    const ID: i32 = clientbound::HELLO;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.server_id);
        write_byte_array(out, self.public_key);
        write_byte_array(out, self.challenge);
        write_bool(out, self.should_authenticate);
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
    /// 皮肤等属性。离线模式下是空的。
    pub properties: &'a [Property],
    /// 每次服务端启动时生成的会话 ID，从 26.3 开始才有这个字段。
    pub session_id: Uuid,
}

impl ClientboundPacket for LoginFinished<'_> {
    const ID: i32 = clientbound::LOGIN_FINISHED;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_uuid(out, self.uuid);
        write_string(out, self.name);
        write_properties(out, self.properties);
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
            properties: &[],
            session_id,
        };
        let mut body = Vec::new();
        packet.write_body(&mut body);

        assert_eq!(&body[body.len() - 16..], session_id.as_bytes());
        assert_eq!(body[body.len() - 17], 0, "properties list should be empty");
    }

    #[test]
    fn login_finished_carries_the_skin() {
        let properties = [Property {
            name: "textures".to_owned(),
            value: "ewo=".to_owned(),
            signature: Some("c2ln".to_owned()),
        }];
        let session_id = Uuid::from_u128(0xabcd);
        let mut body = Vec::new();
        LoginFinished {
            uuid: Uuid::from_u128(1),
            name: "Kitten",
            properties: &properties,
            session_id,
        }
        .write_body(&mut body);

        let mut expected = Vec::new();
        write_uuid(&mut expected, Uuid::from_u128(1));
        write_string(&mut expected, "Kitten");
        write_properties(&mut expected, &properties);
        write_uuid(&mut expected, session_id);
        assert_eq!(body, expected);
    }

    #[test]
    fn decodes_key() {
        let mut payload = vec![serverbound::KEY as u8];
        write_byte_array(&mut payload, &[1, 2, 3]);
        write_byte_array(&mut payload, &[4, 5]);

        assert_eq!(
            LoginRequest::decode(&payload),
            Ok(LoginRequest::Key {
                shared_secret: &[1, 2, 3],
                challenge: &[4, 5],
            })
        );
    }

    #[test]
    fn rejects_a_key_packet_with_absurd_lengths() {
        let mut payload = vec![serverbound::KEY as u8];
        write_byte_array(&mut payload, &vec![0; MAX_ENCRYPTED_LEN + 1]);
        write_byte_array(&mut payload, &[4, 5]);

        assert_eq!(
            LoginRequest::decode(&payload),
            Err(DecodeError::InvalidCount(MAX_ENCRYPTED_LEN as i32 + 1))
        );

        // 第二段缺字节也要被认出来。
        let mut payload = vec![serverbound::KEY as u8];
        write_byte_array(&mut payload, &[1, 2, 3]);
        write_varint(&mut payload, 4);
        payload.push(9);
        assert_eq!(
            LoginRequest::decode(&payload),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    #[test]
    fn encodes_login_hello() {
        let mut out = Vec::new();
        append_frame(
            &mut out,
            &LoginHello {
                server_id: "",
                public_key: &[0xaa, 0xbb],
                challenge: &[1, 2, 3, 4],
                should_authenticate: true,
            },
        )
        .unwrap();
        // 帧长 11：包 ID 0x01，空的 serverId，两段字节数组，一个布尔值。
        assert_eq!(
            out,
            [
                0x0b, 0x01, 0x00, 0x02, 0xaa, 0xbb, 0x04, 0x01, 0x02, 0x03, 0x04, 0x01
            ]
        );
    }
}
