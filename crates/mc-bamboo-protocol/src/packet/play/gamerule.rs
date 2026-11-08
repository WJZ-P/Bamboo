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
