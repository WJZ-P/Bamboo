//! `TreeFeature.updateLeaves`：BFS 把树叶的 `distance` 设成到树干的最短距离（198–260 行）。
//!
//! 官方以包围盒内的 `BitSetDiscreteVoxelShape` 记录已处理格子，从所有 trunk 位置出发
//! 按距离逐层扩散；距离 >= 7 的树叶保持配置里的默认 `distance=7`。这一步会改写方块
//! 状态，逐方块对拍必须照抄。
//!
//! Official `TreeFeature.updateLeaves`: a BFS sets each leaf's `distance` to its
//! shortest distance from a log (lines 198–260). Leaves at distance >= 7 keep the
//! configured default `distance=7`. This rewrites block states, so block-for-block
//! comparison requires an exact port.

use std::collections::HashSet;

use mc_bamboo_data::{block_id_of_state, block_tag};

use crate::feature::state::with_property;

use super::{Pos, TreePlace, has_property, property_value};

/// `Direction.values()` 的顺序：DOWN, UP, NORTH, SOUTH, WEST, EAST。
///
/// The order of official `Direction.values()`: DOWN, UP, NORTH, SOUTH, WEST, EAST.
const DIRECTIONS: [Pos; 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// 官方 `TreeFeature.updateLeaves`（198–260 行）。
///
/// Official `TreeFeature.updateLeaves` (lines 198–260).
pub(crate) fn update_leaves(place: &mut TreePlace<'_, '_>) {
    let Some(bounds) = BoundingBox::encapsulating(place.trunks.iter().chain(place.foliage.iter()))
    else {
        return;
    };
    let mut shape: HashSet<Pos> = HashSet::new();
    // decorationSet / rootPositions 在橡树里都是空，官方的初始 fill 没有格子。
    // decorationSet / rootPositions are empty for oak; the initial fill has no cells.
    let mut to_check: Vec<HashSet<Pos>> = (0..7).map(|_| HashSet::new()).collect();
    to_check[0].extend(place.trunks.iter().copied());

    let mut smallest_distance = 0_usize;
    loop {
        while smallest_distance >= 7 || !to_check[smallest_distance].is_empty() {
            if smallest_distance >= 7 {
                return;
            }
            let Some(&pos) = to_check[smallest_distance].iter().next() else {
                break;
            };
            to_check[smallest_distance].remove(&pos);
            if bounds.is_inside(pos) {
                if smallest_distance != 0 {
                    let state = place.get_block(pos);
                    let distance = smallest_distance.to_string();
                    if let Some(updated) = with_property(state, "distance", &distance) {
                        place
                            .world
                            .set_block_with_heightmaps(pos.0, pos.1, pos.2, updated);
                    }
                }
                shape.insert(pos);
                for dir in DIRECTIONS {
                    let neighbor = (pos.0 + dir.0, pos.1 + dir.1, pos.2 + dir.2);
                    if bounds.is_inside(neighbor) && !shape.contains(&neighbor) {
                        let state = place.get_block(neighbor);
                        if let Some(distance) = optional_distance_at(state) {
                            let new_distance = distance.min(smallest_distance as i32 + 1);
                            if new_distance < 7 {
                                to_check[new_distance as usize].insert(neighbor);
                                smallest_distance = smallest_distance.min(new_distance as usize);
                            }
                        }
                    }
                }
            }
        }
        smallest_distance += 1;
    }
}

/// `LeavesBlock.getOptionalDistanceAt`：`prevents_nearby_leaf_decay` 给 0，树叶给
/// 当前 `distance`，其它方块给 `None`。
///
/// Official `LeavesBlock.getOptionalDistanceAt`: 0 for `prevents_nearby_leaf_decay`,
/// the current `distance` for leaves, `None` otherwise.
fn optional_distance_at(state: u32) -> Option<i32> {
    let id = block_id_of_state(state)? as i32;
    if block_tag("minecraft:prevents_nearby_leaf_decay").is_some_and(|ids| ids.contains(&id)) {
        return Some(0);
    }
    if !has_property(state, "distance") {
        return None;
    }
    property_value(state, "distance")?.parse().ok()
}

/// 所有放置位置的闭区间包围盒（`BoundingBox.encapsulatingPositions`）。
///
/// The inclusive bounding box of all placed positions (`encapsulatingPositions`).
struct BoundingBox {
    min: Pos,
    max: Pos,
}

impl BoundingBox {
    fn encapsulating<'a>(positions: impl Iterator<Item = &'a Pos>) -> Option<Self> {
        let mut min = (i32::MAX, i32::MAX, i32::MAX);
        let mut max = (i32::MIN, i32::MIN, i32::MIN);
        let mut any = false;
        for &(x, y, z) in positions {
            any = true;
            min = (min.0.min(x), min.1.min(y), min.2.min(z));
            max = (max.0.max(x), max.1.max(y), max.2.max(z));
        }
        any.then_some(Self { min, max })
    }

    /// `BoundingBox.isInside`：各轴闭区间。
    ///
    /// Official `BoundingBox.isInside`: inclusive on each axis.
    fn is_inside(&self, pos: Pos) -> bool {
        pos.0 >= self.min.0
            && pos.0 <= self.max.0
            && pos.1 >= self.min.1
            && pos.1 <= self.max.1
            && pos.2 >= self.min.2
            && pos.2 <= self.max.2
    }
}
