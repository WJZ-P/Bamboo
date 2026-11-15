//! `BambooFeature`：竹子（`minecraft:bamboo`）。
//!
//! 官方 `BambooFeature.place` 先检查原点为空且 `Blocks.BAMBOO` 的默认状态
//! `canSurvive`（`BambooStalkBlock.mayPlaceOn`：下面那格在 `#minecraft:supports_bamboo`
//! 里）。然后 `nextInt(12) + 5` 定高度；`nextFloat() < probability` 时以原点为中心
//! 半径 `nextInt(4) + 1` 的圆盘把 `#minecraft:beneath_bamboo_podzol_replaceable`
//! 的地表换成 podzol（`getHeight(WORLD_SURFACE, x, z) - 1`）。最后从原点往上逐格
//! `setBlock` 竹竿，竿数不够 3 格就不放顶端的三格。
//!
//! Official `BambooFeature.place`: check the origin is empty and that bamboo's default
//! state can survive there (`mayPlaceOn` uses `#minecraft:supports_bamboo`), then pick a
//! height of `nextInt(12) + 5`. With probability `probability` it first replaces the
//! surface block (`getHeight(WORLD_SURFACE) - 1`) in a disc of radius `nextInt(4) + 1`
//! with podzol, then stacks bamboo upwards and adds the three-block leafy top when the
//! stalk reached at least three blocks.

use serde_json::Value;

use mc_bamboo_data::{block_by_name, block_id_of_state, block_tag};

use crate::random::WorldgenRandom;

use super::{
    state::with_property,
    world::{FeatureWorld, HeightmapKind},
};

type Pos = (i32, i32, i32);

/// 已解析的 `BambooFeature` 配置。
pub struct BambooFeature {
    probability: f32,
    trunk: u32,
    final_large: u32,
    top_large: u32,
    top_small: u32,
    podzol: u32,
}

impl BambooFeature {
    /// 解析 `probability`，并把官方那四个 `BAMBOO_TRUNK` 派生状态算出来。
    ///
    /// Parse `probability` and derive the four official `BAMBOO_TRUNK` variants.
    pub fn parse(config: &Value) -> Option<Self> {
        let probability = config.get("probability")?.as_f64()? as f32;
        let bamboo = block_by_name("minecraft:bamboo")?.default_state;
        let trunk = with_properties(bamboo, &[("age", "1"), ("leaves", "none"), ("stage", "0")])?;
        Some(Self {
            probability,
            trunk,
            final_large: with_properties(trunk, &[("leaves", "large"), ("stage", "1")])?,
            top_large: with_properties(trunk, &[("leaves", "large")])?,
            top_small: with_properties(trunk, &[("leaves", "small")])?,
            podzol: block_by_name("minecraft:podzol")?.default_state,
        })
    }

    /// 官方 `BambooFeature.place`：一个候选位置。返回值是官方的 `placed > 0`——只要原点
    /// 是空气就算「放了」，哪怕 `canSurvive` 失败（`placed++` 在 `canSurvive` 判断之外）。
    ///
    /// Official `BambooFeature.place` at one candidate. The return value is `placed > 0`:
    /// an empty origin counts as placed even when `canSurvive` fails, because `placed++`
    /// sits outside the `canSurvive` check.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        if !world.is_air(origin.0, origin.1, origin.2) {
            return false;
        }
        if self.can_survive(world, origin) {
            let height = random.next_int(12) + 5;
            if random.next_float() < self.probability {
                self.place_podzol_patch(world, random, origin);
            }
            self.place_stalk(world, origin, height);
        }
        true
    }

    /// `BambooStalkBlock.mayPlaceOn`：下面那格在 `#minecraft:supports_bamboo` 里。
    ///
    /// `BambooStalkBlock.mayPlaceOn`: the block below is in `#minecraft:supports_bamboo`.
    fn can_survive(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        let below = world.get_block(pos.0, pos.1 - 1, pos.2);
        let id = block_id_of_state(below).map_or(-1, |id| id as i32);
        block_tag("minecraft:supports_bamboo").is_some_and(|ids| ids.contains(&id))
    }

    /// `random.nextFloat() < probability` 命中后的圆盘：半径 `nextInt(4) + 1`，
    /// 圆内每格地表（`WORLD_SURFACE - 1`）若在 `#minecraft:beneath_bamboo_podzol_replaceable`
    /// 里就换成 podzol。遍历顺序是 x 外层、z 内层，和官方两个 for 一致。
    ///
    /// The disc after `nextFloat() < probability` hits: radius `nextInt(4) + 1`; each
    /// surface block (`WORLD_SURFACE - 1`) inside in
    /// `#minecraft:beneath_bamboo_podzol_replaceable` turns into podzol. Iteration is
    /// x outer, z inner, matching the official nested loops.
    fn place_podzol_patch(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) {
        let radius = random.next_int(4) + 1;
        for x in origin.0 - radius..=origin.0 + radius {
            for z in origin.2 - radius..=origin.2 + radius {
                let dx = x - origin.0;
                let dz = z - origin.2;
                if dx * dx + dz * dz > radius * radius {
                    continue;
                }
                // `getHeight(WORLD_SURFACE, xx, zz) - 1`；世界里的 `getHeight` 已经加了 1，
                // 所以这里等于 primed 高度图的原值。
                // `getHeight(WORLD_SURFACE, xx, zz) - 1`; the world view already adds one, so
                // this is the raw primed heightmap value.
                let y = world.get_height(HeightmapKind::WorldSurface, x, z) - 1;
                let state = world.get_block(x, y, z);
                let id = block_id_of_state(state).map_or(-1, |id| id as i32);
                let replaceable = block_tag("minecraft:beneath_bamboo_podzol_replaceable")
                    .is_some_and(|ids| ids.contains(&id));
                if replaceable {
                    world.set_block_with_heightmaps(x, y, z, self.podzol);
                }
            }
        }
    }

    /// 从原点往上逐格放竹竿；`bambooPos.y - origin.y >= 3` 时在顶上补三格叶子。
    ///
    /// Stack bamboo upwards; when `bambooPos.y - origin.y >= 3` add the leafy top.
    fn place_stalk(&self, world: &mut FeatureWorld<'_>, origin: Pos, height: i32) {
        let mut y = origin.1;
        for _ in 0..height {
            if !world.is_air(origin.0, y, origin.2) {
                break;
            }
            world.set_block_with_heightmaps(origin.0, y, origin.2, self.trunk);
            y += 1;
        }
        if y - origin.1 >= 3 {
            world.set_block_with_heightmaps(origin.0, y, origin.2, self.final_large);
            y -= 1;
            world.set_block_with_heightmaps(origin.0, y, origin.2, self.top_large);
            y -= 1;
            world.set_block_with_heightmaps(origin.0, y, origin.2, self.top_small);
        }
    }
}

/// 依次改几个属性；任一属性不存在时返回 `None`。
///
/// Apply several properties in order; `None` when one does not exist.
fn with_properties(state: u32, entries: &[(&str, &str)]) -> Option<u32> {
    let mut state = state;
    for (name, value) in entries {
        state = with_property(state, name, value)?;
    }
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_the_exported_bamboo_configs() {
        for probability in [0.0, 0.2] {
            let config: Value = json!({"probability": probability, "type": "minecraft:bamboo"});
            let feature = BambooFeature::parse(&config).unwrap();
            // 竹竿是 age=1 / leaves=none / stage=0。
            // The trunk is age=1 / leaves=none / stage=0.
            assert_eq!(
                with_property(feature.trunk, "age", "1"),
                Some(feature.trunk)
            );
            assert_eq!(
                with_property(feature.final_large, "leaves", "large"),
                Some(feature.final_large)
            );
            assert_eq!(
                with_property(feature.top_small, "leaves", "small"),
                Some(feature.top_small)
            );
            assert_ne!(
                feature.trunk,
                block_by_name("minecraft:bamboo").unwrap().default_state
            );
        }
    }
}
