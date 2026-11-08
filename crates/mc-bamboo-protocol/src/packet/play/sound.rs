//! 世界里的音效和方块事件。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{SoundSource, packets::play::clientbound};

use crate::{
    codec::{write_block_pos, write_f32, write_i32, write_i64, write_varint},
    packet::ClientboundPacket,
};

/// 让客户端在某个位置播一个音效，官方的 `ClientboundSoundPacket`。
///
/// 声音是注册表里的 Holder，网络上是 ID + 1；0 表示后面跟着完整的声音定义，Bamboo 只发注册表里的。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound {
    /// `sound_event` 注册表 ID。
    pub sound: i32,
    pub source: SoundSource,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub volume: f32,
    pub pitch: f32,
    /// 客户端用这个数选同一音效的随机变体。
    pub seed: i64,
}

impl Sound {
    /// 官方 `ClientboundSoundPacket` 的 `LOCATION_ACCURACY`：坐标乘 8 再截断成整数发送。
    const LOCATION_ACCURACY: f64 = 8.0;
}

impl ClientboundPacket for Sound {
    const ID: i32 = clientbound::SOUND;

    fn write_body(&self, out: &mut Vec<u8>) {
        // Holder：注册表 ID 加一，见官方的 `ByteBufCodecs.holder`。
        write_varint(out, self.sound + 1);
        write_varint(out, i32::from(self.source.id()));
        for coordinate in [self.x, self.y, self.z] {
            write_i32(out, (coordinate * Self::LOCATION_ACCURACY) as i32);
        }
        write_f32(out, self.volume);
        write_f32(out, self.pitch);
        write_i64(out, self.seed);
    }
}

/// 一个方块事件，官方的 `ClientboundBlockEventPacket`。音符盒的声音和音符粒子都靠它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEvent {
    pub pos: BlockPos,
    /// `minecraft:block` 注册表 ID。
    pub block: i32,
    pub b0: u8,
    pub b1: u8,
}

impl ClientboundPacket for BlockEvent {
    const ID: i32 = clientbound::BLOCK_EVENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_block_pos(out, self.pos);
        out.push(self.b0);
        out.push(self.b1);
        write_varint(out, self.block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn sound_matches_official_bytes() {
        let packet = Sound {
            sound: 1,
            source: mc_bamboo_data::sounds::BLOCKS,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            volume: 1.0,
            pitch: 1.0,
            seed: 0,
        };
        assert_eq!(
            body(&packet),
            hex(
                "02 04 00 00 00 00 00 00 00 00 00 00 00 00 3f 80 00 00 3f 80 00 00 00 00 00 00 00 00 00 00"
            )
        );
    }

    #[test]
    fn sound_scales_coordinates_and_truncates_toward_zero() {
        let packet = Sound {
            sound: 40,
            source: mc_bamboo_data::sounds::RECORDS,
            x: 1.5,
            y: -0.5,
            z: 0.0625,
            volume: 3.0,
            pitch: 0.5,
            seed: -1,
        };
        assert_eq!(
            body(&packet),
            hex(
                "29 02 00 00 00 0c ff ff ff fc 00 00 00 00 40 40 00 00 3f 00 00 00 ff ff ff ff ff ff ff ff"
            )
        );
    }

    #[test]
    fn block_event_matches_official_bytes() {
        let packet = BlockEvent {
            pos: BlockPos::new(8, -60, 8),
            block: 1,
            b0: 0,
            b1: 0,
        };
        assert_eq!(body(&packet), hex("00 00 02 00 00 00 8f c4 00 00 01"));
    }
}
