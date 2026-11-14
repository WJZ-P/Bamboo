//! `VinesFeature`：藤蔓（`minecraft:vines`）。
//!
//! 官方 `VinesFeature.place` 只有两步：原点必须是空气；按 `Direction.values()`
//! （DOWN, UP, NORTH, SOUTH, WEST, EAST）的顺序找第一个「不是 DOWN 且
//! `VineBlock.isAcceptableNeighbour(level, origin.relative(d), d)` 为真」的方向，
//! 放一格把该方向属性设为 true 的藤蔓。`isAcceptableNeighbour` 就是
//! `MultifaceBlock.canAttachTo`：邻居的支撑形状或碰撞形状在反方向那一面是满的。
//!
//! Official `VinesFeature.place`: the origin must be air, then walk
//! `Direction.values()` (DOWN, UP, NORTH, SOUTH, WEST, EAST) and take the first
//! non-DOWN direction whose neighbour accepts the vine; place one vine with that face
//! property set. `isAcceptableNeighbour` is `MultifaceBlock.canAttachTo`, i.e. the
//! neighbour's support or collision shape is full on the opposite face.

use mc_bamboo_data::{Aabb, block_by_name, collision_shape, support_shape};

use crate::random::WorldgenRandom;

use super::{state::with_property, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// `Direction` 的六个值，顺序即 `Direction.values()`（ordinal 顺序）。
///
/// The six `Direction` values in `Direction.values()` (ordinal) order.
const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// 已解析的 `minecraft:vines` 配置（没有字段）。
///
/// The parsed `minecraft:vines` config (no fields).
pub struct VinesFeature {
    vine: u32,
}

impl VinesFeature {
    /// `MapCodec.unit(VinesFeature::new)`：配置里没有字段。
    ///
    /// `MapCodec.unit(VinesFeature::new)`: the config has no fields.
    pub fn parse(_config: &serde_json::Value) -> Option<Self> {
        Some(Self {
            vine: block_by_name("minecraft:vine")?.default_state,
        })
    }

    /// 官方 `VinesFeature.place`：一个候选位置。
    ///
    /// Official `VinesFeature.place` at one candidate.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        _random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        if !world.is_air(origin.0, origin.1, origin.2) {
            return false;
        }
        for direction in DIRECTIONS {
            // 官方跳过 DOWN，`UP` 也走 `canAttachTo`（不额外要求上面是藤蔓）。
            // The official loop skips DOWN; UP still goes through `canAttachTo`.
            if direction == "down" {
                continue;
            }
            let (dx, dy, dz) = offset(direction);
            if !self.accepts(
                world,
                (origin.0 + dx, origin.1 + dy, origin.2 + dz),
                direction,
            ) {
                continue;
            }
            if let Some(state) = with_property(self.vine, direction, "true") {
                world.set_block_with_heightmaps(origin.0, origin.1, origin.2, state);
                return true;
            }
        }
        false
    }

    /// `MultifaceBlock.canAttachTo(level, direction, neighbourPos, state)`：
    /// `Block.isFaceFull(getBlockSupportShape, opposite) ||
    ///  Block.isFaceFull(getCollisionShape, opposite)`。
    ///
    /// `MultifaceBlock.canAttachTo`: the support or collision shape is full on the
    /// opposite face.
    fn accepts(&self, world: &FeatureWorld<'_>, neighbour: Pos, direction: &str) -> bool {
        let state = world.get_block(neighbour.0, neighbour.1, neighbour.2);
        let opposite = opposite_face(direction);
        face_is_full(support_shape(state), opposite)
            || face_is_full(collision_shape(state), opposite)
    }
}

/// `Direction.getOpposite()` 对应的面名。
///
/// The face name of `Direction.getOpposite()`.
fn opposite_face(direction: &str) -> &'static str {
    match direction {
        "up" => "down",
        "down" => "up",
        "north" => "south",
        "south" => "north",
        "west" => "east",
        _ => "west",
    }
}

/// `Direction` 的方块坐标偏移。
///
/// The block-coordinate offset of a `Direction`.
fn offset(direction: &str) -> (i32, i32, i32) {
    match direction {
        "down" => (0, -1, 0),
        "up" => (0, 1, 0),
        "north" => (0, 0, -1),
        "south" => (0, 0, 1),
        "west" => (-1, 0, 0),
        _ => (1, 0, 0),
    }
}

/// `Block.isFaceFull(shape, direction)`：`getFaceShape(direction)` 是完整方块。
///
/// 官方 `getFaceShape` 先在轴向上的极值处切一片，再判断那片是不是完整方块
/// （`SliceShape.isCubeLike`）。对轴对齐盒子的并集，这等价于：某个盒子在该方向上
/// 贴到边界（`min_y <= 0` 之类），并且它在另两个轴上铺满 1×1。这里按这个等价条件实现。
///
/// `Block.isFaceFull(shape, direction)`: the face shape is a full block. The official
/// `getFaceShape` slices the shape at the extremum along the axis and then checks whether
/// the slice is cube-like. For a union of axis-aligned boxes that means one box touches the
/// boundary on that face and spans the full 1x1 on the other two axes.
pub(crate) fn face_is_full(shape: &[Aabb], direction: &str) -> bool {
    const EPSILON: f64 = 1.0e-7;
    let spans = |lower: f64, upper: f64| lower <= EPSILON && upper >= 1.0 - EPSILON;
    shape.iter().any(|box_| {
        let touches_face = match direction {
            "down" => box_.min_y <= EPSILON,
            "up" => box_.max_y >= 1.0 - EPSILON,
            "north" => box_.min_z <= EPSILON,
            "south" => box_.max_z >= 1.0 - EPSILON,
            "west" => box_.min_x <= EPSILON,
            _ => box_.max_x >= 1.0 - EPSILON,
        };
        let spans_tangents = match direction {
            "down" | "up" => spans(box_.min_x, box_.max_x) && spans(box_.min_z, box_.max_z),
            "north" | "south" => spans(box_.min_x, box_.max_x) && spans(box_.min_y, box_.max_y),
            _ => spans(box_.min_y, box_.max_y) && spans(box_.min_z, box_.max_z),
        };
        touches_face && spans_tangents
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_builds_each_face() {
        let feature = VinesFeature::parse(&serde_json::json!({"type": "minecraft:vines"})).unwrap();
        for direction in ["up", "north", "south", "west", "east"] {
            let state = with_property(feature.vine, direction, "true").unwrap();
            assert_ne!(state, feature.vine);
        }
        // 默认状态所有面都是 false。
        // The default state has every face property false.
        assert!(with_property(feature.vine, "north", "false").is_some());
    }

    #[test]
    fn full_face_detection_matches_whole_blocks() {
        let block = [Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)];
        for direction in ["down", "up", "north", "south", "west", "east"] {
            assert!(face_is_full(&block, direction), "{direction}");
        }
        // 空形状任何面都不满。
        // An empty shape is full on no face.
        assert!(!face_is_full(&[], "up"));
    }
}
