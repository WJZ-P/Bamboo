//! 世界边界的六个 clientbound 包，官方 `net.minecraft.network.protocol.game` 里同名的类。
//!
//! 字段顺序、整数宽度和 `f64` 都照官方 `StreamCodec` 写，包 ID 由生成器导出。

use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_f64, write_varint, write_varlong},
    packet::ClientboundPacket,
};

/// `ClientboundInitializeBorderPacket`：`PlayerList.sendLevelInfo` 的第一个包。
///
/// `ClientboundInitializeBorderPacket`: the first packet of `PlayerList.sendLevelInfo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InitializeBorder {
    pub center_x: f64,
    pub center_z: f64,
    pub old_size: f64,
    pub new_size: f64,
    pub lerp_time: i64,
    pub absolute_max_size: i32,
    pub warning_blocks: i32,
    pub warning_time: i32,
}

impl ClientboundPacket for InitializeBorder {
    const ID: i32 = clientbound::INITIALIZE_BORDER;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_f64(out, self.center_x);
        write_f64(out, self.center_z);
        write_f64(out, self.old_size);
        write_f64(out, self.new_size);
        write_varlong(out, self.lerp_time);
        write_varint(out, self.absolute_max_size);
        write_varint(out, self.warning_blocks);
        write_varint(out, self.warning_time);
    }
}

/// `ClientboundSetBorderCenterPacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetBorderCenter {
    pub center_x: f64,
    pub center_z: f64,
}

impl ClientboundPacket for SetBorderCenter {
    const ID: i32 = clientbound::SET_BORDER_CENTER;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_f64(out, self.center_x);
        write_f64(out, self.center_z);
    }
}

/// `ClientboundSetBorderLerpSizePacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetBorderLerpSize {
    pub old_size: f64,
    pub new_size: f64,
    pub lerp_time: i64,
}

impl ClientboundPacket for SetBorderLerpSize {
    const ID: i32 = clientbound::SET_BORDER_LERP_SIZE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_f64(out, self.old_size);
        write_f64(out, self.new_size);
        write_varlong(out, self.lerp_time);
    }
}

/// `ClientboundSetBorderSizePacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetBorderSize {
    pub size: f64,
}

impl ClientboundPacket for SetBorderSize {
    const ID: i32 = clientbound::SET_BORDER_SIZE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_f64(out, self.size);
    }
}

/// `ClientboundSetBorderWarningDelayPacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetBorderWarningDelay {
    pub warning_delay: i32,
}

impl ClientboundPacket for SetBorderWarningDelay {
    const ID: i32 = clientbound::SET_BORDER_WARNING_DELAY;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.warning_delay);
    }
}

/// `ClientboundSetBorderWarningDistancePacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetBorderWarningDistance {
    pub warning_blocks: i32,
}

impl ClientboundPacket for SetBorderWarningDistance {
    const ID: i32 = clientbound::SET_BORDER_WARNING_DISTANCE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.warning_blocks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        codec::{Reader, write_varint},
        frame::append_frame,
    };

    /// 帧头、包 ID 之后的内容。
    ///
    /// Everything after the frame header and the packet ID.
    fn body(packet: &impl ClientboundPacket) -> Vec<u8> {
        let mut frames = Vec::new();
        append_frame(&mut frames, packet).unwrap();
        let mut reader = Reader::new(&frames);
        reader.varint().unwrap();
        reader.varint().unwrap();
        reader.rest().to_vec()
    }

    #[test]
    fn initialize_border_writes_the_official_field_order() {
        let packet = InitializeBorder {
            center_x: 1.5,
            center_z: -2.5,
            old_size: 100.0,
            new_size: 200.0,
            lerp_time: 300,
            absolute_max_size: 29_999_984,
            warning_blocks: 5,
            warning_time: 300,
        };
        let mut expected = Vec::new();
        expected.extend_from_slice(&1.5f64.to_be_bytes());
        expected.extend_from_slice(&(-2.5f64).to_be_bytes());
        expected.extend_from_slice(&100.0f64.to_be_bytes());
        expected.extend_from_slice(&200.0f64.to_be_bytes());
        write_varlong(&mut expected, 300);
        write_varint(&mut expected, 29_999_984);
        write_varint(&mut expected, 5);
        write_varint(&mut expected, 300);
        assert_eq!(body(&packet), expected);
    }

    #[test]
    fn the_five_change_packets_use_their_own_widths() {
        let mut expected = Vec::new();
        expected.extend_from_slice(&3.0f64.to_be_bytes());
        expected.extend_from_slice(&4.0f64.to_be_bytes());
        assert_eq!(
            body(&SetBorderCenter {
                center_x: 3.0,
                center_z: 4.0
            }),
            expected
        );

        let mut expected = Vec::new();
        expected.extend_from_slice(&10.0f64.to_be_bytes());
        expected.extend_from_slice(&20.0f64.to_be_bytes());
        write_varlong(&mut expected, 40);
        assert_eq!(
            body(&SetBorderLerpSize {
                old_size: 10.0,
                new_size: 20.0,
                lerp_time: 40
            }),
            expected
        );

        assert_eq!(
            body(&SetBorderSize { size: 64.0 }),
            64.0f64.to_be_bytes().to_vec()
        );

        let mut expected = Vec::new();
        write_varint(&mut expected, 15);
        assert_eq!(body(&SetBorderWarningDelay { warning_delay: 15 }), expected);
        assert_eq!(
            body(&SetBorderWarningDistance { warning_blocks: 15 }),
            expected
        );
    }
}
