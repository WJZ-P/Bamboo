//! Status 状态：服务器列表里的信息和延迟测量。

use mc_bamboo_data::packets::status::{clientbound, serverbound};

use super::ClientboundPacket;
use crate::codec::{DecodeError, Reader, write_string};

/// 客户端在 Status 状态发送的数据包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusRequest {
    /// 请求服务器列表里显示的信息。
    Status,
    /// 测量延迟，服务端要把这个数字原样发回去。
    Ping(i64),
}

impl StatusRequest {
    /// 解码一帧的负载，负载以数据包 ID 开头。
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        let request = match reader.varint()? {
            serverbound::STATUS_REQUEST => Self::Status,
            serverbound::PING_REQUEST => Self::Ping(reader.i64()?),
            unknown => return Err(DecodeError::UnknownPacket(unknown)),
        };
        reader.finish()?;
        Ok(request)
    }
}

/// JSON 形式的服务器列表信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusResponse<'a> {
    pub json: &'a str,
}

impl ClientboundPacket for StatusResponse<'_> {
    const ID: i32 = clientbound::STATUS_RESPONSE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.json);
    }
}

/// [`StatusRequest::Ping`] 的回应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PongResponse {
    pub payload: i64,
}

impl ClientboundPacket for PongResponse {
    const ID: i32 = clientbound::PONG_RESPONSE;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.payload.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::append_frame;

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
    }
}
