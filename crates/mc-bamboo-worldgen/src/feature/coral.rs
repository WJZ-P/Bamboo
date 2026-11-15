//! 珊瑚链：`OverlayFeature`、`CoralTreeFeature`、`CoralClawFeature`。
//!
//! 26.3 的 `warm_ocean_vegetation` 是一个 `simple_random_selector`，子项是
//! `coral_tree` / `coral_claw`（内联的 `Feature` 类型，不在 `feature` 注册表里）
//! 和 `coral/<color>_block`（`overlay` 类型）。三者都只是把子 `PlacedFeature`
//! 放到某个位置，随机数的取数顺序完全由子地物和这里的分支决定。
//!
//! Coral chain: `OverlayFeature`, `CoralTreeFeature`, `CoralClawFeature`.
//! In 26.3 `warm_ocean_vegetation` is a `simple_random_selector` over `coral_tree` /
//! `coral_claw` (inline `Feature` types, absent from the `feature` registry) and
//! `coral/<color>_block` (`overlay`). All three just place child `PlacedFeature`s, and the
//! draw order is fixed by the children plus the branches here.

use mc_bamboo_core::Direction;
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    composition::Child,
    placement::{PlaceOutcome, PlacementEnv},
    registry::Registry,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// `Direction.Plane.HORIZONTAL`，官方枚举顺序（NORTH、EAST、SOUTH、WEST）。
/// `Direction.Plane.HORIZONTAL` in official enum order (NORTH, EAST, SOUTH, WEST).
const HORIZONTAL: [Direction; 4] = Direction::HORIZONTAL;

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

/// `Util.getRandom(array, random)`：`array[random.nextInt(array.length)]`。
/// `Util.getRandom(array, random)`: `array[random.nextInt(array.length)]`.
fn get_random<T: Copy>(items: &[T], random: &WorldgenRandom) -> T {
    items[random.next_int(items.len() as i32) as usize]
}

/// `Util.toShuffledList(stream, random)` 用的 Fisher-Yates。
/// The Fisher-Yates behind `Util.toShuffledList(stream, random)`.
fn shuffled<T: Copy>(items: &[T], random: &WorldgenRandom) -> Vec<T> {
    let mut out = items.to_vec();
    for i in (2..=out.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        out.swap(i - 1, swap_to);
    }
    out
}

/// `Direction.Plane.HORIZONTAL.getRandomDirection(random)`。
/// `Direction.Plane.HORIZONTAL.getRandomDirection(random)`.
fn random_horizontal(random: &WorldgenRandom) -> Direction {
    HORIZONTAL[random.next_int(4) as usize]
}

/// `OverlayFeature`：依次放置 `features` 里的每个子 `PlacedFeature`。
///
/// `OverlayFeature`: place every child `PlacedFeature` in order.
pub struct OverlayFeature {
    /// `features` 原样保留；`Child` 借用配置，所以放置时才解析。
    ///
    /// `features` kept verbatim; `Child` borrows the config, so it is parsed at place time.
    features: Vec<Value>,
}

impl OverlayFeature {
    /// 解析：`features` 是 `HolderSet<PlacedFeature>`（数组或单个条目）。
    /// Parse: `features` is a `HolderSet<PlacedFeature>` (array or single entry).
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let features = match object.get("features")? {
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        };
        Some(Self { features })
    }

    /// `OverlayFeature.place`：`placedAny |= placer.place(feature, random, origin)`。
    /// `OverlayFeature.place`: `placedAny |= placer.place(feature, random, origin)`.
    pub fn place(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> PlaceOutcome {
        let mut total = PlaceOutcome {
            supported: true,
            placed: false,
        };
        for feature in &self.features {
            let outcome = place_child_value(feature, registry, world, env, random, origin);
            total.supported &= outcome.supported;
            total.placed |= outcome.placed;
        }
        total
    }
}

/// `CoralTreeFeature`：先立一根 1..3 高的树干，再从顶部向 2..4 个水平方向长分枝。
///
/// `CoralTreeFeature`: grow a 1..3 tall trunk, then 2..4 horizontal branches from the top.
pub struct CoralTree {
    feature: Value,
}

impl CoralTree {
    /// 解析：只有一个 `feature` 字段（子 `PlacedFeature`）。
    /// Parse: a single `feature` field (the child `PlacedFeature`).
    pub fn parse(config: &Value) -> Option<Self> {
        Some(Self {
            feature: config.as_object()?.get("feature")?.clone(),
        })
    }

    /// `CoralTreeFeature.place`。
    /// `CoralTreeFeature.place`.
    pub fn place(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> PlaceOutcome {
        let mut supported = true;
        let mut pos = origin;
        let trunk_height = random.next_int(3) + 1;
        for _ in 0..trunk_height {
            let outcome = place_child_value(&self.feature, registry, world, env, random, pos);
            supported &= outcome.supported;
            if !outcome.placed {
                // 官方在子地物放不下时直接 return true。
                // The official code returns true as soon as a child fails to place.
                return PlaceOutcome {
                    supported,
                    placed: true,
                };
            }
            pos = (pos.0, pos.1 + 1, pos.2);
        }

        let trunk_top = pos;
        let branches = random.next_int(3) + 2;
        let directions = shuffled(&HORIZONTAL, random);
        for &branch in directions.iter().take(branches as usize) {
            let (bx, _, bz) = offset(branch);
            let mut pos = (trunk_top.0 + bx, trunk_top.1, trunk_top.2 + bz);
            let branch_height = random.next_int(5) + 2;
            let mut segment_length = 0;
            for j in 0..branch_height {
                let outcome = place_child_value(&self.feature, registry, world, env, random, pos);
                supported &= outcome.supported;
                if !outcome.placed {
                    break;
                }
                segment_length += 1;
                pos = (pos.0, pos.1 + 1, pos.2);
                if j == 0 || (segment_length >= 2 && random.next_float() < 0.25) {
                    pos = (pos.0 + bx, pos.1, pos.2 + bz);
                    segment_length = 0;
                }
            }
        }
        PlaceOutcome {
            supported,
            placed: true,
        }
    }
}

/// `CoralClawFeature`：先在原点放一格，再向 `clawDirection` 及其两侧长爪。
///
/// `CoralClawFeature`: place at the origin, then grow claws toward `clawDirection` and
/// its two sides.
pub struct CoralClaw {
    feature: Value,
}

impl CoralClaw {
    /// 解析：只有一个 `feature` 字段。
    /// Parse: a single `feature` field.
    pub fn parse(config: &Value) -> Option<Self> {
        Some(Self {
            feature: config.as_object()?.get("feature")?.clone(),
        })
    }

    /// `CoralClawFeature.place`。
    /// `CoralClawFeature.place`.
    pub fn place(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> PlaceOutcome {
        let first = place_child_value(&self.feature, registry, world, env, random, origin);
        if !first.placed {
            return PlaceOutcome {
                supported: first.supported,
                placed: false,
            };
        }
        let mut supported = first.supported;

        let claw = random_horizontal(random);
        let branches = random.next_int(2) + 2;
        let candidates = [claw, claw.clockwise(), claw.counter_clockwise()];
        let order = shuffled(&candidates, random);
        for &branch in order.iter().take(branches as usize) {
            let mut pos = origin;
            let (bx, _, bz) = offset(branch);
            let sideway_length = random.next_int(2) + 1;
            pos = (pos.0 + bx, pos.1, pos.2 + bz);
            let (segment, inway_length) = if branch == claw {
                (claw, random.next_int(3) + 2)
            } else {
                pos = (pos.0, pos.1 + 1, pos.2);
                let segment = get_random(&[branch, Direction::Up], random);
                (segment, random.next_int(3) + 3)
            };

            for _ in 0..sideway_length {
                let outcome = place_child_value(&self.feature, registry, world, env, random, pos);
                supported &= outcome.supported;
                if !outcome.placed {
                    break;
                }
                let (sx, sy, sz) = offset(segment);
                pos = (pos.0 + sx, pos.1 + sy, pos.2 + sz);
            }

            let (sx, sy, sz) = offset(segment);
            pos = (pos.0 - sx, pos.1 - sy, pos.2 - sz);
            pos = (pos.0, pos.1 + 1, pos.2);

            for _ in 0..inway_length {
                let (cx, _, cz) = offset(claw);
                pos = (pos.0 + cx, pos.1, pos.2 + cz);
                let outcome = place_child_value(&self.feature, registry, world, env, random, pos);
                supported &= outcome.supported;
                if !outcome.placed {
                    break;
                }
                if random.next_float() < 0.25 {
                    pos = (pos.0, pos.1 + 1, pos.2);
                }
            }
        }
        PlaceOutcome {
            supported,
            placed: true,
        }
    }
}

/// 把 `feature` 字段（`Holder<PlacedFeature>`）解析成子地物并放置。
/// Resolve the `feature` field (`Holder<PlacedFeature>`) and place it.
fn place_child_value(
    feature: &Value,
    registry: &Registry,
    world: &mut FeatureWorld<'_>,
    env: &mut PlacementEnv<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) -> PlaceOutcome {
    let Some(child) = Child::parse(feature) else {
        return PlaceOutcome::unsupported();
    };
    super::composition::place_child(&child, registry, world, env, random, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_coral_tree_and_claw_configs() {
        let tree: Value = serde_json::json!({
            "feature": {"feature": "minecraft:coral/tube_block", "placement": []},
            "type": "minecraft:coral_tree"
        });
        assert!(CoralTree::parse(&tree).is_some());
        let claw: Value = serde_json::json!({
            "feature": {"feature": "minecraft:coral/tube_block", "placement": []},
            "type": "minecraft:coral_claw"
        });
        assert!(CoralClaw::parse(&claw).is_some());
    }
}
