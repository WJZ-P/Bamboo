//! `StraightTrunkPlacer`：橡树的直树干。
//!
//! 官方 `TrunkPlacer.getTreeHeight` 取两次 `nextInt`，`StraightTrunkPlacer.placeTrunk`
//! 先放 `below_trunk_provider` 的泥土，再逐格放原木，最后给出一个位于树干顶端的
//! `FoliageAttachment`。
//!
//! Official `StraightTrunkPlacer`: `getTreeHeight` consumes two `nextInt` calls, then
//! `placeTrunk` places the below-trunk block, the logs, and returns one
//! `FoliageAttachment` at the trunk top.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{FoliageAttachment, Pos, TreeConfig, TreePlace, place_log, valid_tree_pos};

/// `straight_trunk_placer` 的三个参数。
///
/// The three parameters of `straight_trunk_placer`.
pub struct StraightTrunkPlacer {
    base_height: i32,
    height_rand_a: i32,
    height_rand_b: i32,
}

impl StraightTrunkPlacer {
    /// 解析配置；类型不是 `minecraft:straight_trunk_placer` 或字段缺失返回 `None`。
    ///
    /// Parse the config; `None` for a different type or missing fields.
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        if object.get("type").and_then(Value::as_str) != Some("minecraft:straight_trunk_placer") {
            return None;
        }
        let int = |name: &str| object.get(name).and_then(Value::as_i64).map(|n| n as i32);
        Some(Self {
            base_height: int("base_height")?,
            height_rand_a: int("height_rand_a")?,
            height_rand_b: int("height_rand_b")?,
        })
    }

    /// `TrunkPlacer.getTreeHeight(random)`：两次 `nextInt` 都要照抄（含 `nextInt(1)`）。
    ///
    /// Official `TrunkPlacer.getTreeHeight`: both `nextInt` calls matter, including
    /// the `nextInt(1)` when `height_rand_b` is zero.
    pub fn tree_height(&self, random: &WorldgenRandom) -> i32 {
        self.base_height
            + random.next_int(self.height_rand_a + 1)
            + random.next_int(self.height_rand_b + 1)
    }

    /// `StraightTrunkPlacer.placeTrunk`，返回官方那一个 `FoliageAttachment`。
    ///
    /// Official `StraightTrunkPlacer.placeTrunk`, returning its single attachment.
    pub(crate) fn place_trunk(
        &self,
        place: &mut TreePlace<'_, '_>,
        config: &TreeConfig,
        _random: &WorldgenRandom,
        tree_height: i32,
        origin: Pos,
    ) -> Vec<FoliageAttachment> {
        let below = (origin.0, origin.1 - 1, origin.2);
        // `soil_beneath_tree`：当前方块不在 `cannot_replace_below_tree_trunk` 标签里才放泥土。
        // `soil_beneath_tree`: place dirt only when the block is not in the tag.
        if !super::in_cannot_replace_tag(place.get_block(below)) {
            place.set_trunk(below, config.below_trunk_dirt);
        }
        for y in 0..tree_height {
            let pos = (origin.0, origin.1 + y, origin.2);
            if valid_tree_pos(place.world, pos) {
                place_log(place, config.trunk_state, pos);
            }
        }
        vec![FoliageAttachment {
            pos: (origin.0, origin.1 + tree_height, origin.2),
            radius_offset_xz: 0,
            foliage_height_offset: 0,
            double_trunk: false,
        }]
    }
}
