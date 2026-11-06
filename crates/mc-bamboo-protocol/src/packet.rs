//! Packets of the Handshake, Status and Login states.
//!
//! Packet ids match `reports/packets.json` from the official 26.3 data generator.

use crate::codec::{DecodeError, Reader, write_string};

/// The longest server address the official server accepts in a handshake.
const MAX_SERVER_ADDRESS_CHARS: usize = 255;

/// A packet the server sends to clients.
pub trait ClientboundPacket {
    const ID: i32;

    /// Appends everything that follows the packet id.
    fn write_body(&self, out: &mut Vec<u8>);
}

/// What a client wants to do after the handshake.
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

/// The first packet of every connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handshake<'a> {
    pub protocol_version: i32,
    pub server_address: &'a str,
    pub server_port: u16,
    pub intent: Intent,
}

impl<'a> Handshake<'a> {
    const ID: i32 = 0x00;

    /// Decodes a frame payload, which starts with the packet id.
    pub fn decode(payload: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        let id = reader.varint()?;
        if id != Self::ID {
            return Err(DecodeError::UnknownPacket(id));
        }

        let handshake = Self {
            protocol_version: reader.varint()?,
            server_address: reader.string(MAX_SERVER_ADDRESS_CHARS)?,
            server_port: reader.u16()?,
            intent: Intent::from_id(reader.varint()?)?,
        };
        reader.finish()?;
        Ok(handshake)
    }
}

/// A packet a client sends in the Status state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusRequest {
    /// Asks for the server list entry.
    Status,
    /// Measures latency. The server echoes the number back unchanged.
    Ping(i64),
}

impl StatusRequest {
    const STATUS_ID: i32 = 0x00;
    const PING_ID: i32 = 0x01;

    /// Decodes a frame payload, which starts with the packet id.
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        let request = match reader.varint()? {
            Self::STATUS_ID => Self::Status,
            Self::PING_ID => Self::Ping(reader.i64()?),
            unknown => return Err(DecodeError::UnknownPacket(unknown)),
        };
        reader.finish()?;
        Ok(request)
    }
}

/// The server list entry, as JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusResponse<'a> {
    pub json: &'a str,
}

impl ClientboundPacket for StatusResponse<'_> {
    const ID: i32 = 0x00;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.json);
    }
}

/// The answer to [`StatusRequest::Ping`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PongResponse {
    pub payload: i64,
}

impl ClientboundPacket for PongResponse {
    const ID: i32 = 0x01;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.payload.to_be_bytes());
    }
}

/// Refuses a login. The reason is a text component in JSON form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginDisconnect<'a> {
    pub reason_json: &'a str,
}

impl ClientboundPacket for LoginDisconnect<'_> {
    const ID: i32 = 0x00;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.reason_json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PROTOCOL_VERSION,
        codec::write_varint,
        frame::{MAX_FRAME_LEN, append_frame, find_frame},
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
    fn decodes_status_requests() {
        assert_eq!(StatusRequest::decode(&[0x00]), Ok(StatusRequest::Status));
        assert_eq!(
            StatusRequest::decode(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0x2a]),
            Ok(StatusRequest::Ping(42))
        );
    }

    #[test]
    fn rejects_malformed_status_requests() {
        assert_eq!(
            StatusRequest::decode(&[0x00, 0x00]),
            Err(DecodeError::TrailingBytes(1))
        );
        assert_eq!(
            StatusRequest::decode(&[0x01, 0, 0]),
            Err(DecodeError::UnexpectedEnd)
        );
        assert_eq!(
            StatusRequest::decode(&[0x02]),
            Err(DecodeError::UnknownPacket(2))
        );
    }

    #[test]
    fn encodes_clientbound_packets() {
        let mut out = Vec::new();
        append_frame(&mut out, &StatusResponse { json: "{}" }).unwrap();
        assert_eq!(out, [0x04, 0x00, 0x02, b'{', b'}']);

        out.clear();
        append_frame(&mut out, &PongResponse { payload: 42 }).unwrap();
        assert_eq!(out, [0x09, 0x01, 0, 0, 0, 0, 0, 0, 0, 0x2a]);

        out.clear();
        append_frame(&mut out, &LoginDisconnect { reason_json: "{}" }).unwrap();
        assert_eq!(out, [0x04, 0x00, 0x02, b'{', b'}']);
    }

    /// Network input is attacker-controlled, so no byte sequence may cause a panic.
    #[test]
    fn decoders_survive_arbitrary_bytes() {
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
