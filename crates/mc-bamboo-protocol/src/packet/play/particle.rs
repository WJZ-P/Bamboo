//! 等级粒子包。官方 `ClientboundLevelParticlesPacket.STREAM_CODEC`。

use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_bool, write_f32, write_f64, write_varint},
    packet::ClientboundPacket,
};

/// 随机化方式。`/particle` 只发 `DEFAULT`（0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Randomization {
    Default = 0,
    Alternative = 1,
    AlternativeWithSpeed = 2,
}

/// 一个等级粒子包。`options` 是 `ParticleTypes.STREAM_CODEC` 写出的字节：
/// 类型 ID（VarInt）后面跟着该类型专属的选项。
#[derive(Debug, Clone, PartialEq)]
pub struct LevelParticles<'a> {
    pub options: &'a [u8],
    /// `force` 为真时客户端跳过粒子数量限制，服务端也把广播距离放宽到 512 格。
    pub override_limiter: bool,
    pub always_show: bool,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub x_dist: f32,
    pub y_dist: f32,
    pub z_dist: f32,
    pub max_speed: f32,
    pub count: i32,
    pub randomization: Randomization,
}

impl ClientboundPacket for LevelParticles<'_> {
    const ID: i32 = clientbound::LEVEL_PARTICLES;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.options);
        write_bool(out, self.override_limiter);
        write_bool(out, self.always_show);
        write_f64(out, self.x);
        write_f64(out, self.y);
        write_f64(out, self.z);
        write_f32(out, self.x_dist);
        write_f32(out, self.y_dist);
        write_f32(out, self.z_dist);
        write_f32(out, self.max_speed);
        write_f32(out, self.max_speed);
        write_f32(out, self.max_speed);
        write_varint(out, self.count);
        write_varint(out, self.randomization as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, frame, hex};

    fn packet<'a>(options: &'a [u8], force: bool) -> LevelParticles<'a> {
        LevelParticles {
            options,
            override_limiter: force,
            always_show: false,
            x: 1.5,
            y: 2.5,
            z: 3.5,
            x_dist: 0.25,
            y_dist: 0.5,
            z_dist: 0.75,
            max_speed: 2.0,
            count: 3,
            randomization: Randomization::Default,
        }
    }

    #[test]
    fn simple_particle_matches_the_official_bytes() {
        // 官方 `new ClientboundLevelParticlesPacket(FLAME, false, false, 1.5, 2.5, 3.5,
        // 0.25, 0.5, 0.75, 2.0, 3)` 的编码。
        let expected = hex("27 00 00
             3f f8 00 00 00 00 00 00 40 04 00 00 00 00 00 00 40 0c 00 00 00 00 00 00
             3e 80 00 00 3f 00 00 00 3f 40 00 00
             40 00 00 00 40 00 00 00 40 00 00 00
             03 00");
        assert_eq!(body(&packet(&[0x27], false)), expected);
    }

    #[test]
    fn force_sets_the_second_boolean() {
        let normal = body(&packet(&[0x27], false));
        let forced = body(&packet(&[0x27], true));
        assert_eq!(normal[1], 0);
        assert_eq!(forced[1], 1);
        assert_eq!(normal.len(), forced.len());
    }

    #[test]
    fn frame_writes_the_official_packet_id() {
        assert_eq!(frame(&packet(&[0x27], false))[..2], hex("36 30"));
    }

    #[test]
    fn option_bytes_go_first_and_untouched() {
        let options = [0x15, 0xff, 0xff, 0, 0, 0x3f, 0x80, 0, 0];
        let body = body(&packet(&options, false));
        assert_eq!(&body[..options.len()], &options);
        assert_eq!(body[options.len()], 0);
    }
}
