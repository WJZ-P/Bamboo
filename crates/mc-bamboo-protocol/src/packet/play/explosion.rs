//! 爆炸包。官方 `ClientboundExplodePacket`。

use mc_bamboo_core::Vec3;
use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_bool, write_f32, write_i32, write_varint, write_vec3},
    packet::ClientboundPacket,
};

/// `DEFAULT_EXPLOSION_BLOCK_PARTICLES` 里的一项：一个 `ExplosionParticleInfo` 加它的权重。
///
/// One entry of `DEFAULT_EXPLOSION_BLOCK_PARTICLES`: an `ExplosionParticleInfo` plus its weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockParticle {
    /// 粒子类型注册表 ID（`ParticleTypes.STREAM_CODEC` 写类型 ID）。
    pub particle: i32,
    /// `ExplosionParticleInfo.scaling`。
    pub scaling: f32,
    /// `ExplosionParticleInfo.speed`。
    pub speed: f32,
    /// `Weighted.weight`。
    pub weight: i32,
}

/// 官方 `Level.DEFAULT_EXPLOSION_BLOCK_PARTICLES`：炸掉的方块飞出来的两种粒子。
///
/// `Level.DEFAULT_EXPLOSION_BLOCK_PARTICLES`: the two block particles an explosion throws out.
pub const DEFAULT_EXPLOSION_BLOCK_PARTICLES: &[BlockParticle] = &[
    BlockParticle {
        particle: mc_bamboo_data::registries::particle_type::POOF,
        scaling: 0.5,
        speed: 1.0,
        weight: 1,
    },
    BlockParticle {
        particle: mc_bamboo_data::registries::particle_type::SMOKE,
        scaling: 1.0,
        speed: 1.0,
        weight: 1,
    },
];

/// 一个爆炸包，字段和官方 `STREAM_CODEC` 一一对应。
///
/// A blast packet mirroring the official `STREAM_CODEC` field order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Explode<'a> {
    pub center: Vec3,
    pub radius: f32,
    pub block_count: i32,
    /// 这名玩家自己那份击退向量；`ServerLevel.explode` 从 `getHitPlayers` 取。
    pub player_knockback: Option<Vec3>,
    /// `explosionParticle`：爆炸粒子类型注册表 ID（`EXPLOSION` 或 `EXPLOSION_EMITTER`）。
    pub explosion_particle: i32,
    /// `explosionSound`：`sound_event` 注册表 ID，网络上写成 ID 加一。
    pub explosion_sound: i32,
    /// `blockParticles`：方块粒子加权表。
    pub block_particles: &'a [BlockParticle],
    /// `playSound`：来源实体不静音时才放声音。
    pub play_sound: bool,
}

impl ClientboundPacket for Explode<'_> {
    const ID: i32 = clientbound::EXPLODE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_vec3(out, self.center);
        write_f32(out, self.radius);
        write_i32(out, self.block_count);
        match self.player_knockback {
            Some(knockback) => {
                write_bool(out, true);
                write_vec3(out, knockback);
            }
            None => write_bool(out, false),
        }
        write_varint(out, self.explosion_particle);
        // Holder：注册表 ID 加一。爆炸音效总是注册表里的那一项。
        write_varint(out, self.explosion_sound + 1);
        write_varint(out, self.block_particles.len() as i32);
        for entry in self.block_particles {
            write_varint(out, entry.particle);
            write_f32(out, entry.scaling);
            write_f32(out, entry.speed);
            write_varint(out, entry.weight);
        }
        write_bool(out, self.play_sound);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn explode_matches_the_official_bytes() {
        // 官方 `new ClientboundExplodePacket(center=(1.0, 2.0, 3.0), radius=4.0,
        // blockCount=10, knockback=(0.5, 0.25, -0.25), EXPLOSION_EMITTER,
        // GENERIC_EXPLODE, DEFAULT_EXPLOSION_BLOCK_PARTICLES, playSound=true)` 的编码。
        let packet = Explode {
            center: mc_bamboo_core::Vec3::new(1.0, 2.0, 3.0),
            radius: 4.0,
            block_count: 10,
            player_knockback: Some(mc_bamboo_core::Vec3::new(0.5, 0.25, -0.25)),
            explosion_particle: mc_bamboo_data::registries::particle_type::EXPLOSION_EMITTER,
            explosion_sound: mc_bamboo_data::sounds::sound_event(
                "minecraft:entity.generic.explode",
            )
            .into(),
            block_particles: DEFAULT_EXPLOSION_BLOCK_PARTICLES,
            play_sound: true,
        };
        let expected = hex("3f f0 00 00 00 00 00 00 \
             40 00 00 00 00 00 00 00 \
             40 08 00 00 00 00 00 00 \
             40 80 00 00 \
             00 00 00 0a \
             01 \
             3f e0 00 00 00 00 00 00 3f d0 00 00 00 00 00 00 bf d0 00 00 00 00 00 00 \
             1d \
             c0 05 \
             02 \
             45 3f 00 00 00 3f 80 00 00 01 \
             48 3f 80 00 00 3f 80 00 00 01 \
             01");
        assert_eq!(body(&packet), expected);
    }

    #[test]
    fn absent_knockback_writes_a_false_boolean() {
        let packet = Explode {
            center: mc_bamboo_core::Vec3::new(0.0, 0.0, 0.0),
            radius: 4.0,
            block_count: 0,
            player_knockback: None,
            explosion_particle: mc_bamboo_data::registries::particle_type::EXPLOSION,
            explosion_sound: 703,
            block_particles: DEFAULT_EXPLOSION_BLOCK_PARTICLES,
            play_sound: true,
        };
        let bytes = body(&packet);
        // 24 字节中心 + 4 字节半径 + 4 字节 block_count 之后就是那个 false。
        assert_eq!(bytes[32], 0);
    }

    #[test]
    fn small_explosion_uses_the_small_particle() {
        let packet = Explode {
            center: mc_bamboo_core::Vec3::new(0.0, 0.0, 0.0),
            radius: 1.0,
            block_count: 3,
            player_knockback: None,
            explosion_particle: mc_bamboo_data::registries::particle_type::EXPLOSION,
            explosion_sound: 703,
            block_particles: DEFAULT_EXPLOSION_BLOCK_PARTICLES,
            play_sound: false,
        };
        let bytes = body(&packet);
        // radius 1.0 是 f32，block_count 3 是 i32，然后 knockback false，然后粒子类型。
        assert_eq!(&bytes[24..28], &1.0_f32.to_be_bytes());
        assert_eq!(&bytes[28..32], &3_i32.to_be_bytes());
        assert_eq!(bytes[32], 0);
        assert_eq!(
            bytes[33],
            mc_bamboo_data::registries::particle_type::EXPLOSION as u8
        );
    }
}
