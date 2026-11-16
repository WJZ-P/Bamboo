//! `BlockPileFeature`：村庄 `FeaturePoolElement` 里的南瓜堆。
//!
//! 字节码（`BlockPileFeature.place`）：
//! * `pos.y < level.getMinY() + 5` 直接返回 false；
//! * `width = 2 + random.nextInt(2)`、`depth = 2 + random.nextInt(2)`；
//! * 遍历 `BlockPos.betweenClosed(pos.offset(-width,0,-depth), pos.offset(width,1,depth))`，
//!   顺序由 `BlockPos$4` 给出：x 最内、其次 y、最外 z；
//! * 每格算 `dx = pos.x - cur.x`、`dz = pos.z - cur.z`；
//!   `(dx*dx + dz*dz) as f32 > nextFloat()*10.0 - nextFloat()*6.0` 时**只**再取一次
//!   `nextFloat()`，小于 `0.031` 才尝试放置；否则直接尝试放置；
//! * `tryPlaceBlock`：格子是空气且 `mayPlaceOn` 通过才写
//!   `state_provider.getState(level, random, pos)`（flags 260）。
//! * `mayPlaceOn`：下面一格是 `dirt_path` 时返回 `nextBoolean()`，否则返回该格
//!   `isFaceSturdy(UP)`。
//!
//! `BlockPileFeature` from village `FeaturePoolElement`s. The loop order, short
//! circuits and random draws follow the official bytecode exactly.

use mc_bamboo_data::{blocks, facts, state_facts};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{state_provider::StateProvider, world::FeatureWorld};

type Pos = (i32, i32, i32);

pub struct BlockPileFeature {
    provider: StateProvider,
}

impl BlockPileFeature {
    pub fn parse(config: &Value) -> Option<Self> {
        Some(Self {
            provider: StateProvider::parse(config.get("state_provider")?)?,
        })
    }
}

/// `DIRT_PATH.defaultBlockState()` 的注册表 ID（只比较方块 ID，不比较状态）。
fn is_dirt_path(state: u32) -> bool {
    mc_bamboo_data::block_id_of_state(state) == mc_bamboo_data::block_id_of_state(blocks::DIRT_PATH)
}

pub fn place(
    feature: &BlockPileFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) -> bool {
    if pos.1 < world.min_y + 5 {
        return false;
    }
    let width = 2 + random.next_int(2);
    let depth = 2 + random.next_int(2);
    // `BlockPos$4.computeNext`：index % width → x，index / width % height → y，
    // index / (width*height) → z。height = 2（y 从 0 到 1）。
    for z in -depth..=depth {
        for dy in 0..=1 {
            for x in -width..=width {
                let cur = (pos.0 + x, pos.1 + dy, pos.2 + z);
                let dx = pos.0 - cur.0;
                let dz = pos.2 - cur.2;
                let dist2 = (dx * dx + dz * dz) as f32;
                let threshold = random.next_float() * 10.0 - random.next_float() * 6.0;
                // 字节码 151-185：`fcmpg` + `ifgt` 把 `dist2 > threshold` 跳到
                // 「再取一次 nextFloat，小于 0.031 才尝试」那一支；`<=` 直接尝试。
                // Bytecode 151-185: `dist2 > threshold` takes the extra
                // `nextFloat() < 0.031` branch; `<=` tries to place directly.
                let attempt = if dist2 > threshold {
                    random.next_float() < 0.031
                } else {
                    true
                };
                if attempt {
                    try_place_block(&feature.provider, world, random, cur);
                }
            }
        }
    }
    true
}

fn try_place_block(
    provider: &StateProvider,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) {
    if !world.is_air(pos.0, pos.1, pos.2) {
        return;
    }
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    let may_place = if is_dirt_path(below) {
        random.next_boolean()
    } else {
        state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0
    };
    if !may_place {
        return;
    }
    let Some(state) = provider.get_state(random) else {
        return;
    };
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_the_exported_pile_pumpkin() {
        let config: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN)
            .ok()
            .and_then(|root: Value| {
                root.get("feature")?
                    .get("minecraft:pile_pumpkin")?
                    .clone()
                    .into()
            })
            .unwrap_or_else(|| json!({}));
        assert!(BlockPileFeature::parse(&config).is_some());
    }
}
