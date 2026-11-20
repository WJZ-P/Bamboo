//! `TreeFeature.updateLeaves`：从树干出发按距离分层，把树叶的 `distance` 写好。
//!
//! 官方用 7 个 `HashSet<BlockPos>` 当作按距离分的桶，每次用 `iterator().next()` 取
//! 一个位置处理。同一片树叶可能同时待在两个桶里：它的邻居在它自己被处理之前先被
//! 处理时，会按「邻居距离 + 1」再登记它一次。之后它从更远的桶里再被取出时，
//! `distance` 会被改写成那个更大的值。所以最终结果取决于 `HashSet` 的迭代顺序，
//! 这里用 [`JavaHashSet`] 照原样重放，不做「取最小值」之类的修正。
//!
//! `TreeFeature.updateLeaves`: spreads outwards from the logs and writes each leaf's
//! `distance`.
//!
//! The official code uses seven `HashSet<BlockPos>` as per-distance buckets and takes
//! one position at a time with `iterator().next()`. A leaf can sit in two buckets at
//! once: when a neighbour is processed before the leaf itself, the leaf is registered
//! again at "neighbour distance + 1". When it later comes out of the farther bucket
//! its `distance` is overwritten with that larger value. The outcome therefore depends
//! on `HashSet` iteration order, which [`JavaHashSet`] replays as is, without any
//! "keep the minimum" correction.

use std::{collections::HashSet, sync::OnceLock};

use mc_bamboo_data::{block_id_of_state, block_tag};

use crate::feature::state::with_property;

use super::{Pos, TreePlace, has_property, java_set::JavaHashSet, property_value};

/// `Direction.values()` 的顺序：DOWN、UP、NORTH、SOUTH、WEST、EAST。
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

const MAX_DISTANCE: usize = 7;

/// `TreeFeature.place` 的收尾：算包围盒，跑 `updateLeaves`。
/// 返回官方那个已填充的 `DiscreteVoxelShape` 和包围盒，没有任何位置时返回 `None`。
///
/// The tail of `TreeFeature.place`: compute the bounds and run `updateLeaves`.
/// Returns the filled `DiscreteVoxelShape` and the bounds, or `None` without positions.
pub(crate) fn update_leaves(place: &mut TreePlace<'_, '_>) -> Option<(HashSet<Pos>, BoundingBox)> {
    let bounds = BoundingBox::encapsulating(
        place
            .roots
            .iter()
            .chain(place.trunks.iter())
            .chain(place.foliage.iter())
            .chain(place.decorations.iter()),
    )?;
    let mut shape: HashSet<Pos> = HashSet::new();
    for pos in place.decorations.iter().chain(place.roots.iter()) {
        if bounds.is_inside(pos) {
            shape.insert(pos);
        }
    }
    let mut to_check: Vec<JavaHashSet> = (0..MAX_DISTANCE).map(|_| JavaHashSet::new()).collect();
    // `toCheck.get(0).addAll(logs)`：按 logs 的迭代顺序逐个 add。
    // `toCheck.get(0).addAll(logs)`: adds one by one in the iteration order of logs.
    for pos in place.trunks.iter() {
        to_check[0].insert(pos);
    }

    let mut smallest = 0_usize;
    loop {
        while smallest >= MAX_DISTANCE || !to_check[smallest].is_empty() {
            if smallest >= MAX_DISTANCE {
                return Some((shape, bounds));
            }
            let Some(pos) = to_check[smallest].first() else {
                break;
            };
            to_check[smallest].remove(pos);
            if !bounds.is_inside(pos) {
                continue;
            }
            if smallest != 0 {
                let state = place.get_block(pos);
                // 官方的 `setValue` 在方块没有 `distance` 属性时会抛异常；能进桶的
                // 位置在登记时都有这个属性，这里遇到没有的就原样跳过。
                // Official `setValue` throws without a `distance` property; every
                // registered position had one, so a missing one is simply skipped.
                if let Some(updated) = with_property(state, "distance", &smallest.to_string()) {
                    place.world.set_tree_block(pos.0, pos.1, pos.2, updated);
                }
            }
            shape.insert(pos);
            for dir in DIRECTIONS {
                let neighbor = (pos.0 + dir.0, pos.1 + dir.1, pos.2 + dir.2);
                if !bounds.is_inside(neighbor) || shape.contains(&neighbor) {
                    continue;
                }
                let Some(distance) = optional_distance_at(place.get_block(neighbor)) else {
                    continue;
                };
                let new_distance = (distance as usize).min(smallest + 1);
                if new_distance < MAX_DISTANCE {
                    to_check[new_distance].insert(neighbor);
                    smallest = smallest.min(new_distance);
                }
            }
        }
        smallest += 1;
    }
}

/// `LeavesBlock.getOptionalDistanceAt`：`prevents_nearby_leaf_decay` 给 0，带
/// `distance` 属性的方块给当前值，其它给 `None`。
///
/// Official `LeavesBlock.getOptionalDistanceAt`: 0 for `prevents_nearby_leaf_decay`,
/// the current value for blocks with a `distance` property, `None` otherwise.
fn optional_distance_at(state: u32) -> Option<i32> {
    static TAG: OnceLock<&'static [i32]> = OnceLock::new();
    let prevents =
        TAG.get_or_init(|| block_tag("minecraft:prevents_nearby_leaf_decay").unwrap_or(&[]));
    let id = block_id_of_state(state)? as i32;
    if prevents.contains(&id) {
        return Some(0);
    }
    if !has_property(state, "distance") {
        return None;
    }
    property_value(state, "distance")?.parse().ok()
}

/// 闭区间包围盒（`BoundingBox.encapsulatingPositions`）。
///
/// An inclusive bounding box (`BoundingBox.encapsulatingPositions`).
pub(crate) struct BoundingBox {
    pub min: Pos,
    pub max: Pos,
}

impl BoundingBox {
    fn encapsulating(positions: impl Iterator<Item = Pos>) -> Option<Self> {
        let mut min = (i32::MAX, i32::MAX, i32::MAX);
        let mut max = (i32::MIN, i32::MIN, i32::MIN);
        let mut any = false;
        for (x, y, z) in positions {
            any = true;
            min = (min.0.min(x), min.1.min(y), min.2.min(z));
            max = (max.0.max(x), max.1.max(y), max.2.max(z));
        }
        any.then_some(Self { min, max })
    }

    /// `BoundingBox.isInside`：各轴闭区间。
    ///
    /// Official `BoundingBox.isInside`: inclusive on each axis.
    pub fn is_inside(&self, pos: Pos) -> bool {
        pos.0 >= self.min.0
            && pos.0 <= self.max.0
            && pos.1 >= self.min.1
            && pos.1 <= self.max.1
            && pos.2 >= self.min.2
            && pos.2 <= self.max.2
    }
}
