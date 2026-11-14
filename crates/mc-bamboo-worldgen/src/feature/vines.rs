//! `VinesFeature`：藤蔓地物的 `place`。
//!
//! 官方 `VinesFeature.place`：先判 `isEmptyBlock(origin)`，再按 `Direction.values()`
//! 顺序（跳过 DOWN）找第一个 `VineBlock.isAcceptableNeighbour` 成立的相邻格，
//! 把默认藤蔓状态的对应面属性置 true 后 `setBlock(origin, state, 2)`。
//! `isAcceptableNeighbour` 转发到 `MultifaceBlock.canAttachTo`：相邻方块朝回来的那一面
//! （`direction.getOpposite()`）在支撑形状或碰撞形状上是满的就能贴。
//!
//! `VinesFeature.place` for vines: bail unless `isEmptyBlock(origin)`, then walk
//! `Direction.values()` (skipping DOWN) for the first `VineBlock.isAcceptableNeighbour`;
//! set the matching face property on the default vine state and `setBlock(..., 2)`.
//! `isAcceptableNeighbour` forwards to `MultifaceBlock.canAttachTo`: the neighbour's face
//! toward the origin (`direction.getOpposite()`) must be full in the support or collision shape.

use mc_bamboo_core::Direction;
use mc_bamboo_data::{collision_shape, support_shape};
use serde_json::{Value, json};

use crate::random::WorldgenRandom;

use super::{state::parse_state, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// `Direction.values()` 里除 DOWN 外的五个方向，顺序照官方枚举。
/// The five directions of `Direction.values()` except DOWN, in enum order.
const DIRECTIONS: [Direction; 5] = [
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

/// `BlockPos.relative(direction)` 的位移。
/// The block offset of `BlockPos.relative(direction)`.
fn offset(direction: Direction) -> (i32, i32, i32) {
    match direction {
        Direction::Down => (0, -1, 0),
        Direction::Up => (0, 1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::West => (-1, 0, 0),
        Direction::East => (1, 0, 0),
    }
}

/// `VineBlock.getPropertyForFace(direction)` 的属性名（DOWN 不会走到）。
/// Property name of `VineBlock.getPropertyForFace(direction)` (DOWN is never used).
fn face_property(direction: Direction) -> &'static str {
    match direction {
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
        Direction::Down => "down",
    }
}

/// `MultifaceBlock.canAttachTo(level, direction, neighbourPos, neighbourState)`：
/// 朝 `face` 的支撑形状或碰撞形状是满面。
/// `MultifaceBlock.canAttachTo`: the support or collision shape is full on `face`.
fn can_attach_to(state: u32, face: Direction) -> bool {
    let support = support_shape(state);
    let collision = collision_shape(state);
    mc_bamboo_blocks::shape::is_face_full(support, face)
        || mc_bamboo_blocks::shape::is_face_full(collision, face)
}

/// 已解析的藤蔓地物；`states[d.id()]` 是该面为 true 的状态。
/// Parsed vines feature; `states[d.id()]` is the state with that face set.
pub struct VinesFeature {
    states: [u32; 6],
}

impl VinesFeature {
    /// 配置是 unit codec（`MapCodec.unit`），没有字段；只需预算五个面状态。
    /// The config is a unit codec with no fields; only precompute the five face states.
    pub fn parse(_config: &Value) -> Option<Self> {
        let mut states = [0_u32; 6];
        for &direction in &DIRECTIONS {
            let value = json!({
                "id": "minecraft:vine",
                "properties": { face_property(direction): "true" },
            });
            states[direction.id() as usize] = parse_state(&value).ok()?;
        }
        Some(Self { states })
    }

    /// 一个候选位置。`place` 不消费随机数，`random` 只为统一签名保留。
    /// One candidate. `place` consumes no random; `random` keeps the shared signature.
    pub fn place(&self, world: &mut FeatureWorld<'_>, _random: &WorldgenRandom, pos: Pos) {
        if !world.is_air(pos.0, pos.1, pos.2) {
            return;
        }
        for &direction in &DIRECTIONS {
            let (dx, dy, dz) = offset(direction);
            let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
            let face = direction.opposite();
            if can_attach_to(neighbour, face) {
                world.set_block_with_heightmaps(
                    pos.0,
                    pos.1,
                    pos.2,
                    self.states[direction.id() as usize],
                );
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_all_five_face_states() {
        let feature = VinesFeature::parse(&json!({"type": "minecraft:vines"})).unwrap();
        for &direction in &DIRECTIONS {
            assert_ne!(feature.states[direction.id() as usize], 0);
        }
    }

    #[test]
    fn face_properties_are_distinct() {
        let feature = VinesFeature::parse(&json!({})).unwrap();
        let mut seen = std::collections::HashSet::new();
        for &direction in &DIRECTIONS {
            assert!(seen.insert(feature.states[direction.id() as usize]));
        }
    }
}
