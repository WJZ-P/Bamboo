//! 实体移动：角度和位移的编码，以及持续同步用的数据包。

use mc_bamboo_core::Vec3;
use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_bool, write_f32, write_i16, write_i32, write_varint, write_vec3},
    packet::ClientboundPacket,
};

/// 定点位移的精度：1/4096 格。
const FIXED_POINT_STEPS: f64 = 4096.0;

const LP_ABS_MAX_VALUE: f64 = 1.7179869183e10;
const LP_ABS_MIN_VALUE: f64 = 3.051944088384301e-5;
const LP_MAX_QUANTIZED_VALUE: f64 = 32766.0;
const LP_SCALE_MASK: i64 = 3;
const LP_CONTINUATION_FLAG: i64 = 4;
const LP_X_OFFSET: u32 = 3;
const LP_Y_OFFSET: u32 = 18;
const LP_Z_OFFSET: u32 = 33;

/// 把角度打包成一个字节。全程用 `f32`，向下取整，和 `Mth.packDegrees` 相同。
pub fn pack_degrees(angle: f32) -> u8 {
    (angle * 256.0 / 360.0).floor() as i32 as u8
}

/// Java 的 `Math.round`：`.5` 向正无穷取整。`f64::round` 是远离零，负数的结果不同。
pub fn java_round(value: f64) -> i64 {
    let floor = value.floor();
    (if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }) as i64
}

/// 把一个绝对坐标转成定点数。
pub fn fixed_point(value: f64) -> i64 {
    java_round(value * FIXED_POINT_STEPS)
}

/// 定点位移，单位是 1/4096 格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionDelta {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

impl PositionDelta {
    /// 从 `base` 到 `pos` 的位移。任一轴放不进 `i16`（约 8 格）时返回 `None`，
    /// 这时要改发 [`EntityPositionSync`]。
    ///
    /// 两个坐标分别取整后再相减，这样连续的位移不会累积误差。
    pub fn between(base: Vec3, pos: Vec3) -> Option<Self> {
        Some(Self {
            x: axis_delta(base.x, pos.x)?,
            y: axis_delta(base.y, pos.y)?,
            z: axis_delta(base.z, pos.z)?,
        })
    }

    fn write(self, out: &mut Vec<u8>) {
        write_i16(out, self.x);
        write_i16(out, self.y);
        write_i16(out, self.z);
    }
}

fn axis_delta(base: f64, pos: f64) -> Option<i16> {
    // 坐标来自客户端，极端的值相减会溢出。
    let delta = fixed_point(pos).checked_sub(fixed_point(base))?;
    i16::try_from(delta).ok()
}

/// `move_entity_pos` 和 `move_entity_pos_rot` 里的位移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VecDelta {
    /// 客户端按一步 2 tick 处理。
    Linear(PositionDelta),
    /// 一步。`ticks` 是这一步用的 tick 数。
    Stepped { ticks: i32, delta: PositionDelta },
}

impl VecDelta {
    fn step_count(self) -> i32 {
        match self {
            Self::Linear(_) => 0,
            Self::Stepped { .. } => 1,
        }
    }

    fn write(self, out: &mut Vec<u8>) {
        match self {
            Self::Linear(delta) => delta.write(out),
            Self::Stepped { ticks, delta } => {
                write_varint(out, ticks);
                delta.write(out);
            }
        }
    }
}

/// 写入实体 ID、属性和位移。`onGround` 折叠进属性的最低位。
fn write_entity_delta(out: &mut Vec<u8>, entity_id: i32, delta: VecDelta, on_ground: bool) {
    write_varint(out, entity_id);
    write_varint(out, i32::from(on_ground) | delta.step_count() << 1);
    delta.write(out);
}

/// 实体相对上次同步的位置移动了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveEntityPos {
    pub entity_id: i32,
    pub delta: VecDelta,
    pub on_ground: bool,
}

impl ClientboundPacket for MoveEntityPos {
    const ID: i32 = clientbound::MOVE_ENTITY_POS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_entity_delta(out, self.entity_id, self.delta, self.on_ground);
    }
}

/// 实体移动并转向。角度是 [`pack_degrees`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveEntityPosRot {
    pub entity_id: i32,
    pub delta: VecDelta,
    pub yaw: u8,
    pub pitch: u8,
    pub on_ground: bool,
}

impl ClientboundPacket for MoveEntityPosRot {
    const ID: i32 = clientbound::MOVE_ENTITY_POS_ROT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_entity_delta(out, self.entity_id, self.delta, self.on_ground);
        out.extend_from_slice(&[self.yaw, self.pitch]);
    }
}

/// 实体转向。角度是 [`pack_degrees`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveEntityRot {
    pub entity_id: i32,
    pub yaw: u8,
    pub pitch: u8,
    pub on_ground: bool,
}

impl ClientboundPacket for MoveEntityRot {
    const ID: i32 = clientbound::MOVE_ENTITY_ROT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        // 和带位置的两个包不同，这里 onGround 是单独一个字节，排在角度前面。
        write_bool(out, self.on_ground);
        out.extend_from_slice(&[self.yaw, self.pitch]);
    }
}

/// [`EntityPositionSync`] 的位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionPath {
    Linear(Vec3),
    /// 一步。`ticks` 是这一步用的 tick 数。
    Stepped {
        position: Vec3,
        ticks: i32,
    },
}

impl PositionPath {
    fn write(self, out: &mut Vec<u8>) {
        match self {
            Self::Linear(position) => {
                write_varint(out, 0);
                write_vec3(out, position);
            }
            Self::Stepped { position, ticks } => {
                write_varint(out, 1);
                // 步数。
                write_varint(out, 1);
                write_vec3(out, position);
                write_varint(out, ticks);
            }
        }
    }
}

/// 全量同步实体的位置和朝向。角度是真实角度，不是打包后的字节。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityPositionSync {
    pub entity_id: i32,
    pub path: PositionPath,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl ClientboundPacket for EntityPositionSync {
    const ID: i32 = clientbound::ENTITY_POSITION_SYNC;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        self.path.write(out);
        write_f32(out, self.yaw);
        write_f32(out, self.pitch);
        write_bool(out, self.on_ground);
    }
}

/// 实体的头转动了。角度是 [`pack_degrees`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateHead {
    pub entity_id: i32,
    pub head_yaw: u8,
}

impl ClientboundPacket for RotateHead {
    const ID: i32 = clientbound::ROTATE_HEAD;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        out.push(self.head_yaw);
    }
}

/// 写入一个低精度向量（`LpVec3`）。
///
/// 48 位缓冲区：低 3 位是缩放的低 2 位和续位标记，后面 x、y、z 各 15 位。
/// 线上是前两个字节，再把其余部分按大端 Int 写出，缩放放不进 2 位时最后跟一个 VarInt。
pub fn write_lp_vec3(out: &mut Vec<u8>, value: Vec3) {
    let [x, y, z] = [value.x, value.y, value.z].map(sanitize_lp);
    let largest = x.abs().max(y.abs()).max(z.abs());
    if largest < LP_ABS_MIN_VALUE {
        out.push(0);
        return;
    }

    let scale = largest.ceil() as i64;
    let partial = scale & LP_SCALE_MASK != scale;
    let markers = if partial {
        scale & LP_SCALE_MASK | LP_CONTINUATION_FLAG
    } else {
        scale
    };
    let divisor = scale as f64;
    let buffer = markers
        | pack_lp(x / divisor) << LP_X_OFFSET
        | pack_lp(y / divisor) << LP_Y_OFFSET
        | pack_lp(z / divisor) << LP_Z_OFFSET;

    out.push(buffer as u8);
    out.push((buffer >> 8) as u8);
    write_i32(out, (buffer >> 16) as i32);
    if partial {
        write_varint(out, (scale >> 2) as i32);
    }
}

fn sanitize_lp(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-LP_ABS_MAX_VALUE, LP_ABS_MAX_VALUE)
    }
}

fn pack_lp(value: f64) -> i64 {
    java_round((value * 0.5 + 0.5) * LP_MAX_QUANTIZED_VALUE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    const ENTITY: i32 = 7;
    const DELTA: PositionDelta = PositionDelta {
        x: 1024,
        y: 0,
        z: -512,
    };
    const POSITION: Vec3 = Vec3::new(8.5, 65.0, -3.25);
    const POSITION_HEX: &str = "40 21 00 00 00 00 00 00
                                40 50 40 00 00 00 00 00
                                c0 0a 00 00 00 00 00 00";

    fn lp(x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut out = Vec::new();
        write_lp_vec3(&mut out, Vec3::new(x, y, z));
        out
    }

    fn stepped(ticks: i32, x: i16) -> VecDelta {
        VecDelta::Stepped {
            ticks,
            delta: PositionDelta { x, y: 0, z: 0 },
        }
    }

    #[test]
    fn pack_degrees_matches_the_game() {
        let cases = [
            (0.0, 0x00),
            (45.0, 0x20),
            (90.0, 0x40),
            (-90.0, 0xc0),
            (180.0, 0x80),
            (-180.0, 0x80),
            (179.9, 0x7f),
            (359.9, 0xff),
            (-0.1, 0xff),
            (1.4, 0x00),
            (1.41, 0x01),
        ];
        for (angle, packed) in cases {
            assert_eq!(pack_degrees(angle), packed, "packing {angle}");
        }
    }

    #[test]
    fn java_round_breaks_ties_towards_positive_infinity() {
        let cases = [
            (-2.5, -2),
            (2.5, 3),
            (-0.5, 0),
            (0.5, 1),
            (-1.5, -1),
            (-1.6, -2),
            (1.4, 1),
            (0.499_999_999_999_999_94, 0),
            (f64::NAN, 0),
            (f64::INFINITY, i64::MAX),
            (f64::NEG_INFINITY, i64::MIN),
        ];
        for (value, rounded) in cases {
            assert_eq!(java_round(value), rounded, "rounding {value}");
        }
    }

    #[test]
    fn fixed_point_delta_matches_the_official_example() {
        assert_eq!(fixed_point(8.5), 34816);
        assert_eq!(fixed_point(-3.25), -13312);
        assert_eq!(
            PositionDelta::between(POSITION, Vec3::new(8.75, 65.0, -3.375)),
            Some(DELTA)
        );
    }

    #[test]
    fn delta_rounds_both_coordinates_before_subtracting() {
        // 分别取整是 1 - 0，先相减再取整是 0。
        let base = Vec3::new(0.4 / 4096.0, 0.0, 0.0);
        let pos = Vec3::new(0.6 / 4096.0, 0.0, 0.0);
        assert_eq!(
            PositionDelta::between(base, pos),
            Some(PositionDelta { x: 1, y: 0, z: 0 })
        );
    }

    #[test]
    fn delta_has_the_range_of_a_short_on_every_axis() {
        let origin = Vec3::default();
        let max = 32767.0 / 4096.0;

        assert_eq!(
            PositionDelta::between(origin, Vec3::new(max, -8.0, 0.0)),
            Some(PositionDelta {
                x: 32767,
                y: -32768,
                z: 0
            })
        );
        assert_eq!(
            PositionDelta::between(origin, Vec3::new(8.0, 0.0, 0.0)),
            None
        );
        assert_eq!(
            PositionDelta::between(origin, Vec3::new(0.0, 0.0, -8.001)),
            None
        );
    }

    #[test]
    fn delta_survives_absurd_coordinates() {
        let huge = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let tiny = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        assert_eq!(PositionDelta::between(tiny, huge), None);
        assert_eq!(PositionDelta::between(huge, tiny), None);
    }

    #[test]
    fn lp_vec3_matches_official_bytes() {
        let cases: [(f64, f64, f64, &str); 6] = [
            (0.0, 0.0, 0.0, "00"),
            (1e-5, 0.0, 0.0, "00"),
            (0.5, 0.0, -0.25, "f9 ff 5f fe ff fe"),
            (1.5, -2.25, 3.0, "fb ff ff fc 40 02"),
            (4.0, 0.0, 0.0, "f4 ff 7f fe ff ff 01"),
            (5.0, 0.0, 0.0, "f5 ff 7f fe ff ff 01"),
        ];
        for (x, y, z, expected) in cases {
            assert_eq!(lp(x, y, z), hex(expected), "encoding ({x}, {y}, {z})");
        }
    }

    #[test]
    fn lp_vec3_treats_nan_as_zero() {
        assert_eq!(lp(f64::NAN, f64::NAN, f64::NAN), hex("00"));
    }

    #[test]
    fn move_entity_pos_matches_official_bytes() {
        let linear = MoveEntityPos {
            entity_id: ENTITY,
            delta: VecDelta::Linear(DELTA),
            on_ground: true,
        };
        assert_eq!(body(&linear), hex("07 01 04 00 00 00 fe 00"));

        let stepped_on_ground = MoveEntityPos {
            entity_id: ENTITY,
            delta: VecDelta::Stepped {
                ticks: 2,
                delta: DELTA,
            },
            on_ground: true,
        };
        assert_eq!(body(&stepped_on_ground), hex("07 03 02 04 00 00 00 fe 00"));
    }

    #[test]
    fn move_entity_pos_matches_the_official_trace() {
        let linear_zero = MoveEntityPos {
            entity_id: ENTITY,
            delta: VecDelta::Linear(PositionDelta { x: 0, y: 0, z: 0 }),
            on_ground: false,
        };
        assert_eq!(body(&linear_zero), hex("07 00 0000 0000 0000"));

        let cases = [
            (stepped(2, 0), "07 02 02 0000 0000 0000"),
            (stepped(2, 0x0333), "07 02 02 0333 0000 0000"),
            (stepped(1, 0x019a), "07 02 01 019a 0000 0000"),
        ];
        for (delta, expected) in cases {
            let packet = MoveEntityPos {
                entity_id: ENTITY,
                delta,
                on_ground: false,
            };
            assert_eq!(body(&packet), hex(expected));
        }
    }

    #[test]
    fn move_entity_pos_rot_matches_official_bytes() {
        let packet = MoveEntityPosRot {
            entity_id: ENTITY,
            delta: VecDelta::Stepped {
                ticks: 2,
                delta: DELTA,
            },
            yaw: 0x40,
            pitch: 0xe0,
            on_ground: false,
        };
        assert_eq!(body(&packet), hex("07 02 02 04 00 00 00 fe 00 40 e0"));

        let trace = MoveEntityPosRot {
            entity_id: ENTITY,
            delta: stepped(2, 0x0333),
            yaw: 0x4e,
            pitch: 0x00,
            on_ground: false,
        };
        assert_eq!(body(&trace), hex("07 02 02 0333 0000 0000 4e 00"));
    }

    #[test]
    fn move_entity_rot_writes_on_ground_before_the_angles() {
        let packet = MoveEntityRot {
            entity_id: ENTITY,
            yaw: 0x40,
            pitch: 0xe0,
            on_ground: true,
        };
        assert_eq!(body(&packet), hex("07 01 40 e0"));

        let trace = MoveEntityRot {
            entity_id: ENTITY,
            yaw: 0x40,
            pitch: 0x00,
            on_ground: false,
        };
        assert_eq!(body(&trace), hex("07 00 40 00"));
    }

    #[test]
    fn entity_position_sync_matches_official_bytes() {
        let linear = EntityPositionSync {
            entity_id: ENTITY,
            path: PositionPath::Linear(POSITION),
            yaw: 90.0,
            pitch: 0.0,
            on_ground: true,
        };
        assert_eq!(
            body(&linear),
            hex(&format!("07 00 {POSITION_HEX} 42 b4 00 00 00 00 00 00 01"))
        );

        let stepped = EntityPositionSync {
            path: PositionPath::Stepped {
                position: POSITION,
                ticks: 2,
            },
            ..linear
        };
        assert_eq!(
            body(&stepped),
            hex(&format!(
                "07 01 01 {POSITION_HEX} 02 42 b4 00 00 00 00 00 00 01"
            ))
        );
    }

    #[test]
    fn rotate_head_is_an_id_and_an_angle() {
        let packet = RotateHead {
            entity_id: ENTITY,
            head_yaw: 0x40,
        };
        assert_eq!(body(&packet), hex("07 40"));
    }
}
