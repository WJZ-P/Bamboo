//! Handshake 状态：每条连接发来的第一个数据包。

use mc_bamboo_data::packets::handshake::serverbound;

use crate::codec::{DecodeError, Reader};

/// 原版服务端允许握手包携带的最长服务器地址。
pub const MAX_SERVER_ADDRESS_CHARS: usize = 255;
/// Spigot 把上限提高到 `Short.MAX_VALUE`，好让 legacy 转发的皮肤属性塞进服务器地址。
pub const MAX_LEGACY_SERVER_ADDRESS_CHARS: usize = 32767;

/// 客户端握手之后想做的事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Status,
    Login,
    Transfer,
}

impl Intent {
    fn from_id(id: i32) -> Result<Self, DecodeError> {
        match id {
            1 => Ok(Self::Status),
            2 => Ok(Self::Login),
            3 => Ok(Self::Transfer),
            unknown => Err(DecodeError::UnknownIntent(unknown)),
        }
    }
}

/// 每条连接的第一个数据包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handshake<'a> {
    pub protocol_version: i32,
    pub server_address: &'a str,
    pub server_port: u16,
    pub intent: Intent,
}

impl<'a> Handshake<'a> {
    /// 解码一帧的负载，负载以数据包 ID 开头。
    pub fn decode(payload: &'a [u8]) -> Result<Self, DecodeError> {
        Self::decode_with_address_limit(payload, MAX_SERVER_ADDRESS_CHARS)
    }

    /// 和 [`decode`](Self::decode) 相同，但允许更长的服务器地址。
    /// 代理把玩家 IP、UUID 和皮肤塞在地址里，官方后端会放开这个上限。
    pub fn decode_with_address_limit(
        payload: &'a [u8],
        max_address_chars: usize,
    ) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        let id = reader.varint()?;
        if id != serverbound::INTENTION {
            return Err(DecodeError::UnknownPacket(id));
        }

        let handshake = Self {
            protocol_version: reader.varint()?,
            server_address: reader.string(max_address_chars)?,
            server_port: reader.u16()?,
            intent: Intent::from_id(reader.varint()?)?,
        };
        reader.finish()?;
        Ok(handshake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PROTOCOL_VERSION,
        codec::{write_string, write_varint},
    };

    fn handshake_payload(address: &str, intent: i32) -> Vec<u8> {
        let mut payload = vec![0x00];
        write_varint(&mut payload, PROTOCOL_VERSION);
        write_string(&mut payload, address);
        payload.extend_from_slice(&25565u16.to_be_bytes());
        write_varint(&mut payload, intent);
        payload
    }

    #[test]
    fn decodes_a_handshake() {
        let payload = handshake_payload("localhost", 1);
        assert_eq!(
            Handshake::decode(&payload),
            Ok(Handshake {
                protocol_version: 777,
                server_address: "localhost",
                server_port: 25565,
                intent: Intent::Status,
            })
        );
    }

    #[test]
    fn decodes_every_intent() {
        for (id, intent) in [
            (1, Intent::Status),
            (2, Intent::Login),
            (3, Intent::Transfer),
        ] {
            let payload = handshake_payload("localhost", id);
            assert_eq!(Handshake::decode(&payload).unwrap().intent, intent);
        }
    }

    #[test]
    fn rejects_an_unknown_intent() {
        let payload = handshake_payload("localhost", 4);
        assert_eq!(
            Handshake::decode(&payload),
            Err(DecodeError::UnknownIntent(4))
        );
    }

    #[test]
    fn rejects_a_handshake_with_another_packet_id() {
        let mut payload = handshake_payload("localhost", 1);
        payload[0] = 0x01;
        assert_eq!(
            Handshake::decode(&payload),
            Err(DecodeError::UnknownPacket(1))
        );
    }

    #[test]
    fn rejects_a_handshake_with_trailing_bytes() {
        let mut payload = handshake_payload("localhost", 1);
        payload.push(0);
        assert_eq!(
            Handshake::decode(&payload),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn enforces_the_server_address_limit() {
        let longest = "a".repeat(MAX_SERVER_ADDRESS_CHARS);
        let payload = handshake_payload(&longest, 1);
        assert!(Handshake::decode(&payload).is_ok());

        let too_long = "a".repeat(MAX_SERVER_ADDRESS_CHARS + 1);
        let payload = handshake_payload(&too_long, 1);
        assert_eq!(
            Handshake::decode(&payload),
            Err(DecodeError::StringTooLong {
                max_chars: MAX_SERVER_ADDRESS_CHARS
            })
        );
    }

    #[test]
    fn forwarded_addresses_may_be_longer_than_a_normal_handshake() {
        let address = format!(
            "localhost\0 127.0.0.1\0 00000000000000000000000000000000\0 [{}]",
            "x".repeat(300)
        );
        let payload = handshake_payload(&address, 2);

        assert_eq!(
            Handshake::decode(&payload),
            Err(DecodeError::StringTooLong {
                max_chars: MAX_SERVER_ADDRESS_CHARS
            })
        );
        assert_eq!(
            Handshake::decode_with_address_limit(&payload, MAX_LEGACY_SERVER_ADDRESS_CHARS)
                .unwrap()
                .server_address,
            address
        );
    }

    /// 网络输入不可信，任何字节序列都不能让解码器 panic。
    #[test]
    fn decoders_survive_arbitrary_bytes() {
        use crate::{
            frame::{MAX_FRAME_LEN, find_frame},
            packet::status::StatusRequest,
        };

        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next_byte = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_be_bytes()[0]
        };

        for round in 0..200_000 {
            let len = round % 48;
            let input: Vec<u8> = (0..len).map(|_| next_byte()).collect();
            let _ = find_frame(&input, MAX_FRAME_LEN);
            let _ = Handshake::decode(&input);
            let _ = StatusRequest::decode(&input);
        }
    }
}
