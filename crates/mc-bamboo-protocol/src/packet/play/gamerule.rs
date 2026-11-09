//! 游戏规则界面用的 `game_rule_values`。
//!
//! The `game_rule_values` packet for the in-world game-rules screen.

use mc_bamboo_data::packets::play::clientbound;

use super::ClientboundPacket;
use crate::codec::{write_string, write_varint};

/// `ClientboundGameRuleValuesPacket`：标识符到序列化值。
///
/// `ClientboundGameRuleValuesPacket`: identifier to serialized value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameRuleValues<'a> {
    pub entries: &'a [(&'a str, &'a str)],
}

impl ClientboundPacket for GameRuleValues<'_> {
    const ID: i32 = clientbound::GAME_RULE_VALUES;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entries.len() as i32);
        for (identifier, value) in self.entries {
            write_string(out, identifier);
            write_string(out, value);
        }
    }
}

/// `ClientboundTransferPacket`：让客户端连到另一台服务器。
///
/// `ClientboundTransferPacket`: asks the client to connect to another server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer<'a> {
    pub host: &'a str,
    pub port: i32,
}

impl ClientboundPacket for Transfer<'_> {
    const ID: i32 = clientbound::TRANSFER;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.host);
        write_varint(out, self.port);
    }
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    use crate::packet::test_util::{body, frame, hex};

    #[test]
    fn transfer_writes_host_and_port() {
        let packet = Transfer {
            host: "localhost",
            port: 25565,
        };
        // 包 ID 132 写成 varint 是 0x84 0x01。"localhost" 九个字节，端口 25565 是 0xdd 0xc7 0x01。
        assert_eq!(
            frame(&packet),
            hex("0f 84 01 09 6c 6f 63 61 6c 68 6f 73 74 dd c7 01")
        );
        assert_eq!(body(&Transfer { host: "", port: 1 }), hex("00 01"));
    }
}
