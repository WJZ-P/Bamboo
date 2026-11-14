//! `BlueIceFeature`（`minecraft:blue_ice`）：在冻洋水面附近把 packed ice 长成蓝冰。
//!
//! `place` 先按海平面、水和邻格 packed ice 判定，再固定 200 次带随机偏移的扩散；
//! 每次迭代消费的 `nextInt` 次数取决于 `xzDiff`，必须照官方顺序。
//!
//! `BlueIceFeature` (`minecraft:blue_ice`): grows blue ice near packed ice in frozen
//! oceans. `place` checks sea level, water and a packed-ice neighbour, then runs a
//! fixed 200-iteration random-walk; each iteration's `nextInt` draws must match the
//! official order.

use mc_bamboo_data::blocks;
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    dripstone_utils::{DIRECTION_OFFSETS, is_air, same_block},
    world::FeatureWorld,
};

/// `BlueIceFeature` 是无配置的 `MapCodec.unit`。
/// `BlueIceFeature` is a configless `MapCodec.unit`.
#[derive(Debug, Clone, Copy)]
pub struct BlueIceFeature;

impl BlueIceFeature {
    pub fn parse(_config: &Value) -> Result<Self, String> {
        Ok(Self)
    }
}

/// `BlueIceFeature.place`。
/// `BlueIceFeature.place`.
pub fn place(
    _feature: &BlueIceFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;
    if oy > world.sea_level - 1 {
        return false;
    }
    if !same_block(world.get_block(ox, oy, oz), blocks::WATER)
        && !same_block(world.get_block(ox, oy - 1, oz), blocks::WATER)
    {
        return false;
    }

    // `Direction.values()` 跳过 DOWN（下标 0），其余任一邻格是 packed ice 即可。
    // Iterate `Direction.values()` skipping DOWN (index 0); any packed-ice neighbour.
    let mut found_packed_ice = false;
    for &(dx, dy, dz) in DIRECTION_OFFSETS.iter().skip(1) {
        if same_block(
            world.get_block(ox + dx, oy + dy, oz + dz),
            blocks::PACKED_ICE,
        ) {
            found_packed_ice = true;
            break;
        }
    }
    if !found_packed_ice {
        return false;
    }

    world.set_block_state(ox, oy, oz, blocks::BLUE_ICE);

    for _ in 0..200 {
        let y_off = random.next_int(5) - random.next_int(6);
        let mut xz_diff = 3;
        if y_off < 2 {
            // Java 整数除法向零截断，负数也如此（Rust `/` 同）。
            // Java integer division truncates toward zero, also for negatives (Rust `/` matches).
            xz_diff += y_off / 2;
        }
        if xz_diff >= 1 {
            // `origin.offset(nextInt - nextInt, yOff, nextInt - nextInt)`：x 两次、z 两次。
            // `origin.offset(nextInt - nextInt, yOff, nextInt - nextInt)`: two draws for x, two for z.
            let dx = random.next_int(xz_diff) - random.next_int(xz_diff);
            let dz = random.next_int(xz_diff) - random.next_int(xz_diff);
            let (px, py, pz) = (ox + dx, oy + y_off, oz + dz);
            let place_state = world.get_block(px, py, pz);
            if is_air(place_state)
                || same_block(place_state, blocks::WATER)
                || same_block(place_state, blocks::PACKED_ICE)
                || same_block(place_state, blocks::ICE)
            {
                // 内层 `Direction.values()` 全部 6 个方向，只问是否有蓝冰邻格，无随机。
                // Inner `Direction.values()` loops all 6 directions, no randomness.
                for &(ddx, ddy, ddz) in &DIRECTION_OFFSETS {
                    if same_block(
                        world.get_block(px + ddx, py + ddy, pz + ddz),
                        blocks::BLUE_ICE,
                    ) {
                        world.set_block_state(px, py, pz, blocks::BLUE_ICE);
                        break;
                    }
                }
            }
        }
    }
    true
}
