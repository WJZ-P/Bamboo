//! `SpikeFeature`（`minecraft:spike`，主世界的 `ice_spike`）：在积雪上长冰刺。
//!
//! 随机消费顺序里有两处短路：`width > 1 && nextInt(60)` 和锥体边缘的
//! `!(nextFloat() > 0.75F)`，必须照官方的 `||` / `&&` 短路求值。
//!
//! `SpikeFeature` (`minecraft:spike`, the overworld `ice_spike`): grows an ice spike
//! on snow. Two short-circuits govern random draws: `width > 1 && nextInt(60)` and
//! the cone-edge `!(nextFloat() > 0.75F)`; both must follow official `||` / `&&`.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate, dripstone_utils::is_air, state::parse_state,
    world::FeatureWorld,
};

/// `SpikeFeature` 的配置。
/// `SpikeFeature` configuration.
#[derive(Debug, Clone)]
pub struct SpikeFeature {
    pub state: u32,
    pub can_place_on: BlockPredicate,
    pub can_replace: BlockPredicate,
}

impl SpikeFeature {
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

    pub fn supported(&self) -> bool {
        self.can_place_on.is_supported() && self.can_replace.is_supported()
    }
}

/// `SpikeFeature.place`。
/// `SpikeFeature.place`.
pub fn place(
    spike: &SpikeFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    mut origin: (i32, i32, i32),
) -> bool {
    // 先落到非空气的第一格（官方 `while`）。
    // Descend to the first non-air cell (official `while`).
    while is_air(world.get_block(origin.0, origin.1, origin.2)) && origin.1 > world.min_y + 2 {
        origin.1 -= 1;
    }
    if !spike.can_place_on.test(world, origin) {
        return false;
    }

    origin.1 += random.next_int(4);
    let height = random.next_int(4) + 7;
    let width = height / 4 + random.next_int(2);
    // `width > 1 && nextInt(60) == 0`：width<=1 时不取这次随机数。
    // `width > 1 && nextInt(60) == 0`: no draw when width<=1.
    if width > 1 && random.next_int(60) == 0 {
        origin.1 += 10 + random.next_int(30);
    }

    for y_off in 0..height {
        let scale = (1.0_f32 - (y_off as f32) / (height as f32)) * (width as f32);
        let new_width = scale.ceil() as i32;
        for xo in -new_width..=new_width {
            let dx = (xo as f32).abs() - 0.25_f32;
            for zo in -new_width..=new_width {
                let dz = (zo as f32).abs() - 0.25_f32;
                // 官方是 `!(dx²+dz² > scale²)`，这里操作数都有限，等价于 `<=`。
                // Official is `!(dx²+dz² > scale²)`; all operands are finite, so `<=` is equal.
                let center_or_inside = (xo == 0 && zo == 0) || dx * dx + dz * dz <= scale * scale;
                if center_or_inside {
                    let not_on_border =
                        xo != -new_width && xo != new_width && zo != -new_width && zo != new_width;
                    // `||` 短路：不在边缘就不消费 nextFloat。
                    // `||` short-circuit: no nextFloat draw off the border.
                    let pass = not_on_border || random.next_float() <= 0.75_f32;
                    if pass {
                        let positive = (origin.0 + xo, origin.1 + y_off, origin.2 + zo);
                        let state = world.get_block(positive.0, positive.1, positive.2);
                        if is_air(state) || spike.can_replace.test(world, positive) {
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
                            if is_air(state) || spike.can_replace.test(world, negative) {
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
        }
    }

    // `width - 1` 夹到 [0,1]，等价于官方的两段 if。
    // `width - 1` clamped to [0,1], equivalent to the official two-branch if.
    let pillar_width = (width - 1).clamp(0, 1);
    for xo in -pillar_width..=pillar_width {
        for zo in -pillar_width..=pillar_width {
            let mut cursor = (origin.0 + xo, origin.1 - 1, origin.2 + zo);
            let mut run_length = 50;
            // 四个角用 `nextInt(5)` 改初始段长。
            // The four corners draw `nextInt(5)` for the initial run length.
            if xo.abs() == 1 && zo.abs() == 1 {
                run_length = random.next_int(5);
            }
            while cursor.1 > 50 {
                let state = world.get_block(cursor.0, cursor.1, cursor.2);
                if !is_air(state) && !spike.can_replace.test(world, cursor) && state != spike.state
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
