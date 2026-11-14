//! `SpikeFeature`（冰刺）。
//!
//! 照 `SpikeFeature.place`：先把原点下移穿过空气直到落在实心方块上（要过 `can_place_on`），
//! 然后向上取一个 4 以内的偏移，按 `height` / `width` 造一个上下对称的锥体，边界格按
//! 3/4 概率取随机决定要不要放；锥体下面再往下延伸一段柱脚（`runLength` 逐格衰减，
//! 到 0 时随机跳过几格）。
//!
//! `SpikeFeature.place`: walk the origin down through air until it lands on a solid block
//! (which must pass `can_place_on`), offset upward by up to 4, build a vertically symmetric
//! cone from `height` / `width` (edge cells survive three times out of four via a short-
//! circuiting random draw), then extend a base pillar downward whose `runLength` decays and
//! occasionally skips a few cells.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{block_predicate::BlockPredicate, state::parse_state, world::FeatureWorld};

/// `SpikeFeature` 的配置。
#[derive(Debug, Clone)]
pub struct SpikeFeatureFeature {
    pub state: u32,
    pub can_place_on: BlockPredicate,
    pub can_replace: BlockPredicate,
}

impl SpikeFeatureFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("spike feature must be an object")?;
        Ok(Self {
            state: parse_state(object.get("state").ok_or("spike has no state")?)?,
            can_place_on: BlockPredicate::parse(
                object
                    .get("can_place_on")
                    .ok_or("spike has no can_place_on")?,
            ),
            can_replace: BlockPredicate::parse(
                object
                    .get("can_replace")
                    .ok_or("spike has no can_replace")?,
            ),
        })
    }

    /// 嵌套谓词是否都已移植。
    ///
    /// Whether every nested predicate is ported.
    pub fn supported(&self) -> bool {
        self.can_place_on.is_supported() && self.can_replace.is_supported()
    }
}

/// `Mth.abs(int)` / `Mth.abs(float)`。
fn abs_i32(value: i32) -> i32 {
    value.abs()
}

/// `Mth.ceil(float)`。
fn ceil_f32(value: f32) -> i32 {
    value.ceil() as i32
}

/// `SpikeFeature.place`。
///
/// `SpikeFeature.place`.
pub fn place(
    spike: &SpikeFeatureFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let mut origin = origin;
    while is_air(world, origin) && origin.1 > world.min_y + 2 {
        origin.1 -= 1;
    }
    if !spike.can_place_on.test(world, origin) {
        return false;
    }
    origin.1 += random.next_int(4);
    let height = random.next_int(4) + 7;
    let width = height / 4 + random.next_int(2);
    if width > 1 && random.next_int(60) == 0 {
        origin.1 += 10 + random.next_int(30);
    }
    for y_off in 0..height {
        let scale = (1.0_f32 - y_off as f32 / height as f32) * width as f32;
        let new_width = ceil_f32(scale);
        for xo in -new_width..=new_width {
            let dx = abs_i32(xo) as f32 - 0.25_f32;
            for zo in -new_width..=new_width {
                let dz = abs_i32(zo) as f32 - 0.25_f32;
                let inside_circle = xo == 0 && zo == 0 || dx * dx + dz * dz <= scale * scale;
                // 官方是 `(insideCircle) && (keep)`：`insideCircle` 为假时 `&&` 短路，
                // **不取** `keep` 里的随机数，所以这里必须先判圆内再求 `keep`。
                // The official code is `(insideCircle) && (keep)`: a short-circuited
                // `insideCircle` skips the random draw inside `keep`, so the circle test
                // must come first.
                if !inside_circle {
                    continue;
                }
                let keep =
                    xo != -new_width && xo != new_width && zo != -new_width && zo != new_width
                        || random.next_float() <= 0.75_f32;
                if !keep {
                    continue;
                }
                let positive = (origin.0 + xo, origin.1 + y_off, origin.2 + zo);
                let state = world.get_block(positive.0, positive.1, positive.2);
                if is_air_state(state) || spike.can_replace.test(world, positive) {
                    world.set_block_with_heightmaps(
                        positive.0,
                        positive.1,
                        positive.2,
                        spike.state,
                    );
                }
                if y_off != 0 && new_width > 1 {
                    let negative = (origin.0 + xo, origin.1 - y_off, origin.2 + zo);
                    let state = world.get_block(negative.0, negative.1, negative.2);
                    if is_air_state(state) || spike.can_replace.test(world, negative) {
                        world.set_block_with_heightmaps(
                            negative.0,
                            negative.1,
                            negative.2,
                            spike.state,
                        );
                    }
                }
            }
        }
    }

    let pillar_width = (width - 1).clamp(0, 1);

    for xo in -pillar_width..=pillar_width {
        for zo in -pillar_width..=pillar_width {
            let mut cursor = (origin.0 + xo, origin.1 - 1, origin.2 + zo);
            let mut run_length = 50;
            if abs_i32(xo) == 1 && abs_i32(zo) == 1 {
                run_length = random.next_int(5);
            }
            while cursor.1 > 50 {
                let state = world.get_block(cursor.0, cursor.1, cursor.2);
                if !is_air_state(state)
                    && !spike.can_replace.test(world, cursor)
                    && state != spike.state
                {
                    break;
                }
                world.set_block_with_heightmaps(cursor.0, cursor.1, cursor.2, spike.state);
                cursor.1 -= 1;
                run_length -= 1;
                if run_length <= 0 {
                    cursor.1 -= random.next_int(5) + 1;
                    run_length = random.next_int(5);
                }
            }
        }
    }
    true
}

/// `WorldGenLevel.isEmptyBlock`。
fn is_air(world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
    is_air_state(world.get_block(pos.0, pos.1, pos.2))
}

fn is_air_state(state: u32) -> bool {
    mc_bamboo_data::state_facts(state) & mc_bamboo_data::facts::AIR != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_ice_spike() {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        let features = root
            .get("feature")
            .and_then(Value::as_object)
            .expect("feature registry");
        let config = features
            .get("minecraft:ice_spike")
            .expect("ice_spike config");
        let spike = SpikeFeatureFeature::parse(config).expect("parses");
        assert!(spike.supported());
    }
}
