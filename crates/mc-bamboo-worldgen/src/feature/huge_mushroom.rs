//! `huge_brown_mushroom` 和 `huge_red_mushroom`：照官方 26.3 的
//! `AbstractHugeMushroomFeature` / `HugeBrownMushroomFeature` / `HugeRedMushroomFeature`
//! 逐行翻译。
//!
//! `huge_brown_mushroom` and `huge_red_mushroom`, translated line by line from official
//! 26.3 `AbstractHugeMushroomFeature` / `HugeBrownMushroomFeature` /
//! `HugeRedMushroomFeature`.

use std::sync::OnceLock;

use serde_json::Value;

use mc_bamboo_data::{block_id_of_state, block_tag, facts, state_facts};

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate, state::with_property, state_provider::StateProvider,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 已解析的巨型蘑菇。
///
/// A parsed huge mushroom.
pub struct HugeMushroomFeature {
    red: bool,
    cap_provider: StateProvider,
    stem_provider: StateProvider,
    foliage_radius: i32,
    can_place_on: BlockPredicate,
}

impl HugeMushroomFeature {
    pub fn parse(kind: &str, config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let red = match kind {
            "minecraft:huge_red_mushroom" => true,
            "minecraft:huge_brown_mushroom" => false,
            _ => return None,
        };
        let can_place_on = BlockPredicate::parse(object.get("can_place_on")?);
        if !can_place_on.is_supported() {
            return None;
        }
        Some(Self {
            red,
            cap_provider: StateProvider::parse(object.get("cap_provider")?)?,
            stem_provider: StateProvider::parse(object.get("stem_provider")?)?,
            foliage_radius: object
                .get("foliage_radius")
                .and_then(Value::as_i64)
                .map_or(2, |n| n as i32),
            can_place_on,
        })
    }

    /// 官方 `AbstractHugeMushroomFeature.place`。
    ///
    /// Official `AbstractHugeMushroomFeature.place`.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        // `getTreeHeight`。
        let mut tree_height = random.next_int(3) + 4;
        if random.next_int(12) == 0 {
            tree_height *= 2;
        }
        if !self.is_valid_position(world, origin, tree_height) {
            return false;
        }
        if self.red {
            self.make_red_cap(world, random, origin, tree_height);
        } else {
            self.make_brown_cap(world, random, origin, tree_height);
        }
        // `placeTrunk`。
        for dy in 0..tree_height {
            if let Some(stem) = self.stem_provider.get_state(world, random, origin) {
                place_mushroom_block(world, (origin.0, origin.1 + dy, origin.2), stem);
            }
        }
        true
    }

    /// 官方 `isValidPosition`。
    ///
    /// Official `isValidPosition`.
    fn is_valid_position(&self, world: &FeatureWorld<'_>, origin: Pos, tree_height: i32) -> bool {
        let y = origin.1;
        let max_y = world.min_y + world.height - 1;
        if y < world.min_y + 1 || y + tree_height + 1 > max_y {
            return false;
        }
        if !self
            .can_place_on
            .test(world, (origin.0, origin.1 - 1, origin.2))
        {
            return false;
        }
        for dy in 0..=tree_height {
            let radius = self.radius_for_height(tree_height, dy);
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let state = world.get_block(origin.0 + dx, origin.1 + dy, origin.2 + dz);
                    if state_facts(state) & facts::AIR == 0 && !is_leaves(state) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 官方 `getTreeRadiusForHeight(-1, -1, foliageRadius, yo)`。`isValidPosition` 传进来
    /// 的 `treeHeight` 就是 -1，所以红蘑菇那两个条件永远不成立，半径恒为 0。
    ///
    /// Official `getTreeRadiusForHeight(-1, -1, foliageRadius, yo)`. `isValidPosition`
    /// really passes -1 for `treeHeight`, so neither red-mushroom condition can hold
    /// and the radius is always 0.
    fn radius_for_height(&self, _tree_height: i32, yo: i32) -> i32 {
        if self.red {
            let tree_height = -1;
            if (yo < tree_height && yo >= tree_height - 3) || yo == tree_height {
                self.foliage_radius
            } else {
                0
            }
        } else if yo <= 3 {
            0
        } else {
            self.foliage_radius
        }
    }

    /// 官方 `HugeBrownMushroomFeature.makeCap`。
    ///
    /// Official `HugeBrownMushroomFeature.makeCap`.
    fn make_brown_cap(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
        tree_height: i32,
    ) {
        let r = self.foliage_radius;
        for dx in -r..=r {
            for dz in -r..=r {
                let (min_x, max_x, min_z, max_z) = (dx == -r, dx == r, dz == -r, dz == r);
                let x_edge = min_x || max_x;
                let z_edge = min_z || max_z;
                if x_edge && z_edge {
                    continue;
                }
                let west = min_x || z_edge && dx == 1 - r;
                let east = max_x || z_edge && dx == r - 1;
                let north = min_z || x_edge && dz == 1 - r;
                let south = max_z || x_edge && dz == r - 1;
                let Some(state) = self.cap_provider.get_state(world, random, origin) else {
                    continue;
                };
                let state = with_sides(
                    state,
                    &[
                        ("west", west),
                        ("east", east),
                        ("north", north),
                        ("south", south),
                    ],
                );
                place_mushroom_block(
                    world,
                    (origin.0 + dx, origin.1 + tree_height, origin.2 + dz),
                    state,
                );
            }
        }
    }

    /// 官方 `HugeRedMushroomFeature.makeCap`。
    ///
    /// Official `HugeRedMushroomFeature.makeCap`.
    fn make_red_cap(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
        tree_height: i32,
    ) {
        for dy in tree_height - 3..=tree_height {
            let radius = if dy < tree_height {
                self.foliage_radius
            } else {
                self.foliage_radius - 1
            };
            let center = self.foliage_radius - 2;
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let x_edge = dx == -radius || dx == radius;
                    let z_edge = dz == -radius || dz == radius;
                    if dy < tree_height && x_edge == z_edge {
                        continue;
                    }
                    let Some(state) = self.cap_provider.get_state(world, random, origin) else {
                        continue;
                    };
                    let state = with_sides(
                        state,
                        &[
                            ("up", dy >= tree_height - 1),
                            ("west", dx < -center),
                            ("east", dx > center),
                            ("north", dz < -center),
                            ("south", dz > center),
                        ],
                    );
                    place_mushroom_block(
                        world,
                        (origin.0 + dx, origin.1 + dy, origin.2 + dz),
                        state,
                    );
                }
            }
        }
    }
}

/// 官方只在状态带齐这几个属性时才改；少一个就整组不改。
///
/// The official code only rewrites the state when it has every listed property;
/// missing one leaves the whole state untouched.
fn with_sides(state: u32, sides: &[(&str, bool)]) -> u32 {
    let mut updated = state;
    for (name, value) in sides {
        match with_property(updated, name, if *value { "true" } else { "false" }) {
            Some(next) => updated = next,
            None => return state,
        }
    }
    updated
}

/// 官方 `placeMushroomBlock`：原来是空气或 `replaceable_by_mushrooms` 才放。
///
/// Official `placeMushroomBlock`: only over air or `replaceable_by_mushrooms`.
fn place_mushroom_block(world: &mut FeatureWorld<'_>, pos: Pos, state: u32) {
    static TAG: OnceLock<&'static [i32]> = OnceLock::new();
    let replaceable =
        TAG.get_or_init(|| block_tag("minecraft:replaceable_by_mushrooms").unwrap_or(&[]));
    let current = world.get_block(pos.0, pos.1, pos.2);
    let in_tag = block_id_of_state(current).is_some_and(|id| replaceable.contains(&(id as i32)));
    if state_facts(current) & facts::AIR != 0 || in_tag {
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
    }
}

fn is_leaves(state: u32) -> bool {
    static TAG: OnceLock<&'static [i32]> = OnceLock::new();
    let leaves = TAG.get_or_init(|| block_tag("minecraft:leaves").unwrap_or(&[]));
    block_id_of_state(state).is_some_and(|id| leaves.contains(&(id as i32)))
}
