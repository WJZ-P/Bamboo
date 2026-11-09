//! 世界里的音效。

use mc_bamboo_data::{SoundSource, packets::play::clientbound};

use crate::{
    codec::{write_bool, write_f32, write_i32, write_i64, write_string, write_varint},
    packet::ClientboundPacket,
};

/// 让客户端停掉正在播的音效，官方 `ClientboundStopSoundPacket`。
///
/// 一个字节的标志：1 表示后面有来源的 VarInt，2 表示后面有音效标识符。
/// 两个都有时先写来源再写标识符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopSound<'a> {
    pub source: Option<SoundSource>,
    pub sound: Option<&'a str>,
}

impl ClientboundPacket for StopSound<'_> {
    const ID: i32 = clientbound::STOP_SOUND;

    fn write_body(&self, out: &mut Vec<u8>) {
        let flags = u8::from(self.source.is_some()) | u8::from(self.sound.is_some()) << 1;
        out.push(flags);
        if let Some(source) = self.source {
            write_varint(out, i32::from(source.id()));
        }
        if let Some(sound) = self.sound {
            write_string(out, sound);
        }
    }
}

/// 让客户端在某个位置播一个音效，官方的 `ClientboundSoundPacket`。
///
/// 声音是注册表里的 Holder：网络上是注册表 ID 加一。`inline` 有值时写 0，再写
/// `SoundEvent.createVariableRangeEvent` 的标识符和空的固定范围。
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    /// `sound_event` 注册表 ID。`inline` 有值时忽略。
    pub sound: i32,
    /// 不在注册表里的音效标识符，官方 `createVariableRangeEvent`。
    pub inline: Option<String>,
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
        // Holder：注册表 ID 加一；0 后面是内联的 `SoundEvent`，见 `ByteBufCodecs.holder`。
        if let Some(name) = &self.inline {
            write_varint(out, 0);
            write_string(out, name);
            // 可变范围：`Optional` 空，没有固定距离。
            write_bool(out, false);
        } else {
            write_varint(out, self.sound + 1);
        }
        write_varint(out, i32::from(self.source.id()));
        for coordinate in [self.x, self.y, self.z] {
            write_i32(out, (coordinate * Self::LOCATION_ACCURACY) as i32);
        }
        write_f32(out, self.volume);
        write_f32(out, self.pitch);
        write_i64(out, self.seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn stop_sound_flags_and_fields_match_official_layout() {
        assert_eq!(
            body(&StopSound {
                source: None,
                sound: None
            }),
            hex("00")
        );
        let packet = StopSound {
            source: Some(mc_bamboo_data::sounds::BLOCKS),
            sound: None,
        };
        assert_eq!(body(&packet), hex("01 04"));
        let packet = StopSound {
            source: None,
            sound: Some("minecraft:entity.player.levelup"),
        };
        let bytes = body(&packet);
        assert_eq!(bytes[0], 2);
        assert_eq!(&bytes[2..], b"minecraft:entity.player.levelup");
        let packet = StopSound {
            source: Some(mc_bamboo_data::sounds::RECORDS),
            sound: Some("minecraft:music.overworld"),
        };
        let bytes = body(&packet);
        assert_eq!(bytes[..2], [3, 2]);
        assert_eq!(&bytes[2..4], &[0x19, b'm']);
    }

    #[test]
    fn sound_matches_official_bytes() {
        let packet = Sound {
            sound: 1,
            inline: None,
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
            inline: None,
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
    fn inline_sound_writes_a_direct_holder() {
        let packet = Sound {
            sound: 0,
            inline: Some("minecraft:custom.note".to_owned()),
            source: mc_bamboo_data::sounds::RECORDS,
            x: 0.5,
            y: 0.5,
            z: 0.5,
            volume: 3.0,
            pitch: 1.0,
            seed: 7,
        };
        // 0、标识符、没有固定范围、来源 records。
        let bytes = body(&packet);
        assert_eq!(bytes[0], 0);
        assert!(
            bytes
                .windows(b"minecraft:custom.note".len())
                .any(|window| window == b"minecraft:custom.note")
        );
    }
}
