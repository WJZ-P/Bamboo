//! `BambooFeature`：竹子地物的 `place`。
//!
//! 官方 `BambooFeature.place`：先判 `isEmptyBlock(origin)` 与
//! `BAMBOO.canSurvive`（下面一格在 `supports_bamboo` 标签里），两者都成立才
//! `nextInt(12)+5` 取高度、`nextFloat()` 判是否铺灰化土，然后自底向上放竹竿，
//! 够高时补三节带叶子的顶。`probability` 为 0 的配置（`bamboo_no_podzol`）仍会
//! 消耗一次 `nextFloat()`，只是不进灰化土分支——取数次数必须照抄。
//!
//! `BambooFeature.place`: bail unless `isEmptyBlock(origin)` and `BAMBOO.canSurvive`
//! (the block below is in `supports_bamboo`). Only then roll `nextInt(12)+5` for the
//! height and `nextFloat()` for the podzol branch, place the stalk upward, and cap it
//! with three leafy states when tall enough. A `probability == 0` config still consumes
//! one `nextFloat()` even though it never enters the podzol branch — the draw count is
//! part of the contract.

use mc_bamboo_data::{block_id_of_state, block_tag};
use serde_json::{Value, json};

use crate::random::WorldgenRandom;

use super::{
    state::parse_state,
    world::{FeatureWorld, HeightmapKind},
};

type Pos = (i32, i32, i32);

/// `Blocks.BAMBOO.defaultBlockState().canSurvive`：下面一格是否在 `supports_bamboo`。
/// Whether the block below is in the `supports_bamboo` tag.
fn can_survive(world: &FeatureWorld<'_>, pos: Pos, supports_bamboo: &[i32]) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    block_id_of_state(below).is_some_and(|id| supports_bamboo.contains(&(id as i32)))
}

/// 竹子的四种预置状态。
/// The four preset bamboo states.
struct BambooStates {
    trunk: u32,
    final_large: u32,
    top_large: u32,
    top_small: u32,
    podzol: u32,
}

fn bamboo_state(leaves: &str, stage: &str) -> Option<u32> {
    parse_state(&json!({
        "id": "minecraft:bamboo",
        "properties": { "age": "1", "leaves": leaves, "stage": stage },
    }))
    .ok()
}

/// 已解析的竹子地物。
/// Parsed bamboo feature.
pub struct BambooFeature {
    probability: f32,
    states: BambooStates,
    supports_bamboo: Vec<i32>,
    beneath_podzol: Vec<i32>,
}

impl BambooFeature {
    /// 配置只有一个 `probability` 字段（`bamboo_no_podzol` 为 0，
    /// `bamboo_some_podzol` 为 0.2）。
    /// The only config field is `probability` (0 for `bamboo_no_podzol`, 0.2 for
    /// `bamboo_some_podzol`).
    pub fn parse(config: &Value) -> Option<Self> {
        let probability = config
            .get("probability")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32;
        let podzol = parse_state(&json!({"id": "minecraft:podzol"})).ok()?;
        let states = BambooStates {
            trunk: bamboo_state("none", "0")?,
            final_large: bamboo_state("large", "1")?,
            top_large: bamboo_state("large", "0")?,
            top_small: bamboo_state("small", "0")?,
            podzol,
        };
        let supports_bamboo = block_tag("minecraft:supports_bamboo")?.to_vec();
        let beneath_podzol = block_tag("minecraft:beneath_bamboo_podzol_replaceable")?.to_vec();
        Some(Self {
            probability,
            states,
            supports_bamboo,
            beneath_podzol,
        })
    }

    /// 一个候选位置，照官方 `place` 的取数顺序。
    /// One candidate, following the official `place` draw order.
    pub fn place(&self, world: &mut FeatureWorld<'_>, random: &WorldgenRandom, pos: Pos) {
        // 官方把 `placed++` 放在 `isEmptyBlock` 分支里、但 `canSurvive` 分支外；返回
        // 值 `placed>0` 对 FeaturePlacer 无影响，这里不建模。
        // Official increments `placed` inside the `isEmptyBlock` branch but outside
        // `canSurvive`; the return `placed>0` is ignored by FeaturePlacer.
        if !world.is_air(pos.0, pos.1, pos.2) || !can_survive(world, pos, &self.supports_bamboo) {
            return;
        }

        // 只有两个前置判定都成立，才消耗这两个随机数。
        // Both RNG draws are consumed only after the two guards pass.
        let height = random.next_int(12) + 5;
        if random.next_float() < self.probability {
            let radius = random.next_int(4) + 1;
            for xx in (pos.0 - radius)..=(pos.0 + radius) {
                for zz in (pos.2 - radius)..=(pos.2 + radius) {
                    let xd = xx - pos.0;
                    let zd = zz - pos.2;
                    if xd * xd + zd * zd <= radius * radius {
                        let surface_y = world.get_height(HeightmapKind::WorldSurface, xx, zz) - 1;
                        let state = world.get_block(xx, surface_y, zz);
                        if block_id_of_state(state)
                            .is_some_and(|id| self.beneath_podzol.contains(&(id as i32)))
                        {
                            world.set_block_with_heightmaps(xx, surface_y, zz, self.states.podzol);
                        }
                    }
                }
            }
        }

        let mut y = pos.1;
        for _ in 0..height {
            if !world.is_air(pos.0, y, pos.2) {
                break;
            }
            world.set_block_with_heightmaps(pos.0, y, pos.2, self.states.trunk);
            y += 1;
        }

        // 官方用可变的 `bambooPos` 自顶向下放三节；这里用最终 y 还原。
        // Official uses a mutable `bambooPos` put three states top-down; recovered here.
        if y - pos.1 >= 3 {
            world.set_block_with_heightmaps(pos.0, y, pos.2, self.states.final_large);
            world.set_block_with_heightmaps(pos.0, y - 1, pos.2, self.states.top_large);
            world.set_block_with_heightmaps(pos.0, y - 2, pos.2, self.states.top_small);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_probability_configs() {
        assert!(BambooFeature::parse(&json!({"probability": 0.0})).is_some());
        assert!(BambooFeature::parse(&json!({"probability": 0.2})).is_some());
    }

    #[test]
    fn leafy_states_differ_from_trunk() {
        let feature = BambooFeature::parse(&json!({})).unwrap();
        assert_ne!(feature.states.trunk, feature.states.final_large);
        assert_ne!(feature.states.top_large, feature.states.top_small);
    }
}
