//! `BlueIceFeature`：冻洋里的一簇蓝冰。
//!
//! 照 `BlueIceFeature.place`：原点必须不高于海平面 - 1、自己或正下方是水、且除正下方
//! 之外至少有一个面挨着浮冰（`packed_ice`），才在原点写蓝冰。然后 200 次尝试，每次
//! 先取 `nextInt(5) - nextInt(6)` 当 y 偏移，按偏移收紧水平范围，再取两个
//! `nextInt(xzDiff)` 组成水平偏移；落点必须是空气 / 水 / 浮冰 / 冰，且六邻里有蓝冰
//! 才写蓝冰。
//!
//! `BlueIceFeature.place`: the origin must sit at or below sea level - 1, be water or have
//! water directly below, and touch `packed_ice` on a face other than the bottom, before a
//! blue-ice block is written there. Then 200 attempts: draw `nextInt(5) - nextInt(6)` for a
//! y offset, tighten the horizontal range from it, draw two `nextInt(xzDiff)` for the
//! horizontal offset, and write blue ice only where the target is air / water / packed ice /
//! ice and has blue ice on one of its six faces.

use mc_bamboo_data::blocks;
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::world::FeatureWorld;

/// `BlueIceFeature` 没有配置字段（`MapCodec.unit`）。
///
/// `BlueIceFeature` has no config fields (`MapCodec.unit`).
#[derive(Debug, Clone, Copy)]
pub struct BlueIceFeature;

impl BlueIceFeature {
    pub fn parse(_config: &Value) -> Result<Self, String> {
        Ok(Self)
    }

    /// 没有嵌套配置，恒可对拍。
    ///
    /// No nested config, always comparable.
    pub fn supported(&self) -> bool {
        true
    }
}

/// `Direction.values()`：DOWN, UP, NORTH, SOUTH, WEST, EAST。
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

fn is_block(state: u32, name: &str) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| block.name == name)
}

/// `BlueIceFeature.place`。
///
/// `BlueIceFeature.place`.
pub fn place(
    _feature: &BlueIceFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    if origin.1 > world.sea_level - 1 {
        return false;
    }
    let here = world.get_block(origin.0, origin.1, origin.2);
    let below = world.get_block(origin.0, origin.1 - 1, origin.2);
    if !is_block(here, "minecraft:water") && !is_block(below, "minecraft:water") {
        return false;
    }
    let mut found_packed_ice = false;
    for direction in DIRECTIONS {
        if direction == (0, -1, 0) {
            continue;
        }
        let neighbor = (
            origin.0 + direction.0,
            origin.1 + direction.1,
            origin.2 + direction.2,
        );
        if is_block(
            world.get_block(neighbor.0, neighbor.1, neighbor.2),
            "minecraft:packed_ice",
        ) {
            found_packed_ice = true;
            break;
        }
    }
    if !found_packed_ice {
        return false;
    }

    world.set_block_with_heightmaps(origin.0, origin.1, origin.2, blocks::BLUE_ICE);

    for _ in 0..200 {
        // 官方的取数顺序：`nextInt(5)`、`nextInt(6)`，然后才是两个 `nextInt(xzDiff)`。
        // Official draw order: `nextInt(5)`, `nextInt(6)`, then the two `nextInt(xzDiff)`.
        let y_off = random.next_int(5) - random.next_int(6);
        let mut xz_diff = 3;
        if y_off < 2 {
            // Java 的 `/` 对负数向零取整（`-1 / 2 == 0`），Rust 的 `/` 同样截断，不能用
            // `div_euclid`（它向下取整，`-1.div_euclid(2) == -1`）。
            // Java's `/` truncates toward zero for negatives (`-1 / 2 == 0`); Rust's `/`
            // truncates the same way, so `div_euclid` must not be used here.
            xz_diff += y_off / 2;
        }
        if xz_diff < 1 {
            continue;
        }
        let place_pos = (
            origin.0 + random.next_int(xz_diff) - random.next_int(xz_diff),
            origin.1 + y_off,
            origin.2 + random.next_int(xz_diff) - random.next_int(xz_diff),
        );
        let place_state = world.get_block(place_pos.0, place_pos.1, place_pos.2);
        if !(mc_bamboo_data::state_facts(place_state) & mc_bamboo_data::facts::AIR != 0
            || is_block(place_state, "minecraft:water")
            || is_block(place_state, "minecraft:packed_ice")
            || is_block(place_state, "minecraft:ice"))
        {
            continue;
        }
        for direction in DIRECTIONS {
            let neighbor = (
                place_pos.0 + direction.0,
                place_pos.1 + direction.1,
                place_pos.2 + direction.2,
            );
            if is_block(
                world.get_block(neighbor.0, neighbor.1, neighbor.2),
                "minecraft:blue_ice",
            ) {
                world.set_block_with_heightmaps(
                    place_pos.0,
                    place_pos.1,
                    place_pos.2,
                    blocks::BLUE_ICE,
                );
                break;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blue_ice_has_no_config_fields() {
        let config: Value = serde_json::json!({"type": "minecraft:blue_ice"});
        assert!(BlueIceFeature::parse(&config).is_ok());
    }
}
