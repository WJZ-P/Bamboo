//! Login 状态：确认玩家身份。

use mc_bamboo_data::packets::login::{clientbound, serverbound};
use uuid::Uuid;

use super::{
    ClientboundPacket,
    profile::{ProfileProperty, write_profile_properties},
};
use crate::codec::{DecodeError, Reader, write_string, write_uuid, write_varint};

/// 玩家名的最大长度。
pub const MAX_PLAYER_NAME_CHARS: usize = 16;

/// 客户端在 Login 状态发送的数据包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginRequest<'a> {
    /// 登录请求，带着玩家名。
    Hello { name: &'a str, uuid: Uuid },
    /// 对登录插件询问的回应。`data` 为 `None` 表示客户端不认识这个频道。
    PluginResponse {
        message_id: i32,
        data: Option<&'a [u8]>,
    },
    /// 客户端已经收到登录成功，准备进入 Configuration 状态。
    Acknowledged,
}

impl<'a> LoginRequest<'a> {
    /// 解码一帧的负载，负载以数据包 ID 开头。
    pub fn decode(payload: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        match reader.varint()? {
            serverbound::HELLO => {
                let hello = Self::Hello {
                    name: reader.string(MAX_PLAYER_NAME_CHARS)?,
                    uuid: reader.uuid()?,
                };
                reader.finish()?;
                Ok(hello)
            }
            serverbound::CUSTOM_QUERY_ANSWER => {
                let message_id = reader.varint()?;
                // 回应里先是一个布尔，表示有没有数据，之后是裸的负载字节。
                let data = if reader.bool()? {
                    Some(reader.rest())
                } else {
                    None
                };
                Ok(Self::PluginResponse { message_id, data })
            }
            serverbound::LOGIN_ACKNOWLEDGED => {
                reader.finish()?;
                Ok(Self::Acknowledged)
            }
            unknown => Err(DecodeError::UnknownPacket(unknown)),
        }
    }
}

/// 询问客户端（代理）要转发的玩家信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginPluginRequest<'a> {
    pub message_id: i32,
    pub channel: &'a str,
    pub data: &'a [u8],
}

impl ClientboundPacket for LoginPluginRequest<'_> {
    const ID: i32 = clientbound::CUSTOM_QUERY;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.message_id);
        write_string(out, self.channel);
        out.extend_from_slice(self.data);
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
    /// 皮肤等属性。离线模式没有属性。
    pub properties: &'a [ProfileProperty],
    /// 每次服务端启动时生成的会话 ID，从 26.3 开始才有这个字段。
    pub session_id: Uuid,
}

impl ClientboundPacket for LoginFinished<'_> {
    const ID: i32 = clientbound::LOGIN_FINISHED;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_uuid(out, self.uuid);
        write_string(out, self.name);
        write_profile_properties(out, self.properties);
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
    fn login_finished_carries_the_skin_properties() {
        let properties = [ProfileProperty {
            name: "textures".to_owned(),
            value: "value".to_owned(),
            signature: Some("signature".to_owned()),
        }];
        let packet = LoginFinished {
            uuid: Uuid::from_u128(1),
            name: "Kitten",
            properties: &properties,
            session_id: Uuid::from_u128(2),
        };
        let mut body = Vec::new();
        packet.write_body(&mut body);

        let mut expected = Vec::new();
        write_uuid(&mut expected, Uuid::from_u128(1));
        write_string(&mut expected, "Kitten");
        write_varint(&mut expected, 1);
        write_string(&mut expected, "textures");
        write_string(&mut expected, "value");
        crate::codec::write_bool(&mut expected, true);
        write_string(&mut expected, "signature");
        write_uuid(&mut expected, Uuid::from_u128(2));
        assert_eq!(body, expected);
    }

    #[test]
    fn encodes_a_login_plugin_request() {
        let mut out = Vec::new();
        append_frame(
            &mut out,
            &LoginPluginRequest {
                message_id: 7,
                channel: "velocity:player_info",
                data: &[4],
            },
        )
        .unwrap();

        let mut reader = Reader::new(&out);
        let frame_len = reader.varint().unwrap();
        assert_eq!(frame_len as usize, out.len() - 1);
        assert_eq!(reader.varint(), Ok(clientbound::CUSTOM_QUERY));
        assert_eq!(reader.varint(), Ok(7));
        assert_eq!(reader.string(32767), Ok("velocity:player_info"));
        assert_eq!(reader.rest(), [4]);
    }

    #[test]
    fn decodes_plugin_responses_with_and_without_data() {
        let mut present = vec![serverbound::CUSTOM_QUERY_ANSWER as u8, 7, 1, 0xaa, 0xbb];
        assert_eq!(
            LoginRequest::decode(&present),
            Ok(LoginRequest::PluginResponse {
                message_id: 7,
                data: Some(&[0xaa, 0xbb]),
            })
        );

        present.pop();
        present.pop();
        present[2] = 0;
        assert_eq!(
            LoginRequest::decode(&present),
            Ok(LoginRequest::PluginResponse {
                message_id: 7,
                data: None,
            })
        );
    }
}
