//! `RootPlacer`：26.3 只有 `mangrove_root_placer` 一种，照官方 `MangroveRootPlacer`
//! 逐行翻译。
//!
//! 树干不从地面起，而是抬高 `trunk_offset_y` 格；四个水平方向各模拟一条往下、往外
//! 延伸的树根，任何一条走不通，整棵树都不放。
//!
//! `RootPlacer`: 26.3 has only `mangrove_root_placer`, translated line by line from
//! official `MangroveRootPlacer`.
//!
//! The trunk starts `trunk_offset_y` blocks above the ground; each horizontal direction
//! simulates one root that runs down and outwards, and if any of them cannot be
//! completed the whole tree is not placed.

use serde_json::Value;

use mc_bamboo_data::{block_id_of_state, registries, state_fluid};

use crate::{
    feature::{
        provider::IntProvider, state::with_property, state_provider::StateProvider,
        world::FeatureWorld,
    },
    random::WorldgenRandom,
};

use super::{HORIZONTAL, Pos, TreePlace, has_property, is_air, trunk::holder_set, valid_tree_pos};

/// 已解析的 `mangrove_root_placer`。
///
/// A parsed `mangrove_root_placer`.
pub struct RootPlacer {
    trunk_offset_y: IntProvider,
    root_provider: StateProvider,
    /// `above_root_placement`：`(provider, chance)`。
    above_root: Option<(StateProvider, f32)>,
    can_grow_through: Vec<i32>,
    muddy_roots_in: Vec<i32>,
    muddy_roots_provider: StateProvider,
    max_root_width: i32,
    max_root_length: i32,
    random_skew_chance: f32,
}

impl RootPlacer {
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        if object.get("type")?.as_str()? != "minecraft:mangrove_root_placer" {
            return None;
        }
        let above_root = match object.get("above_root_placement") {
            Some(above) => Some((
                StateProvider::parse(above.get("above_root_provider")?)?,
                above.get("above_root_placement_chance")?.as_f64()? as f32,
            )),
            None => None,
        };
        let mangrove = object.get("mangrove_root_placement")?;
        let int = |name: &str| mangrove.get(name).and_then(Value::as_i64).map(|n| n as i32);
        Some(Self {
            trunk_offset_y: IntProvider::parse(object.get("trunk_offset_y")?).ok()?,
            root_provider: StateProvider::parse(object.get("root_provider")?)?,
            above_root,
            can_grow_through: holder_set(mangrove.get("can_grow_through")?)?,
            muddy_roots_in: holder_set(mangrove.get("muddy_roots_in")?)?,
            muddy_roots_provider: StateProvider::parse(mangrove.get("muddy_roots_provider")?)?,
            max_root_width: int("max_root_width")?,
            max_root_length: int("max_root_length")?,
            random_skew_chance: mangrove.get("random_skew_chance")?.as_f64()? as f32,
        })
    }

    /// 官方 `RootPlacer.getTrunkOrigin`。
    ///
    /// Official `RootPlacer.getTrunkOrigin`.
    pub fn trunk_origin(&self, origin: Pos, random: &WorldgenRandom) -> Pos {
        (
            origin.0,
            origin.1 + self.trunk_offset_y.sample(random),
            origin.2,
        )
    }

    /// 官方 `MangroveRootPlacer.placeRoots`；返回 `false` 时整棵树不放。
    ///
    /// Official `MangroveRootPlacer.placeRoots`; `false` cancels the whole tree.
    pub(crate) fn place_roots(
        &self,
        place: &mut TreePlace<'_, '_>,
        random: &WorldgenRandom,
        origin: Pos,
        trunk_origin: Pos,
    ) -> bool {
        let mut root_positions: Vec<Pos> = Vec::new();
        for y in origin.1..trunk_origin.1 {
            if !self.can_place_root(place.world, (origin.0, y, origin.2)) {
                return false;
            }
        }
        root_positions.push((trunk_origin.0, trunk_origin.1 - 1, trunk_origin.2));
        for direction in HORIZONTAL {
            let pos = (
                trunk_origin.0 + direction.0,
                trunk_origin.1,
                trunk_origin.2 + direction.1,
            );
            let mut in_direction = Vec::new();
            let complete = self.simulate_roots(
                place.world,
                random,
                pos,
                direction,
                trunk_origin,
                &mut in_direction,
                0,
            );
            if !complete {
                return false;
            }
            root_positions.extend(in_direction);
            root_positions.push(pos);
        }
        for pos in root_positions {
            self.place_root(place, random, pos);
        }
        true
    }

    /// 官方 `MangroveRootPlacer.simulateRoots`。
    ///
    /// Official `MangroveRootPlacer.simulateRoots`.
    #[allow(clippy::too_many_arguments)]
    fn simulate_roots(
        &self,
        world: &FeatureWorld<'_>,
        random: &WorldgenRandom,
        root_pos: Pos,
        direction: (i32, i32),
        root_origin: Pos,
        root_positions: &mut Vec<Pos>,
        layer: i32,
    ) -> bool {
        if layer == self.max_root_length || root_positions.len() as i32 > self.max_root_length {
            return false;
        }
        for pos in self.potential_root_positions(root_pos, direction, random, root_origin) {
            if self.can_place_root(world, pos) {
                root_positions.push(pos);
                let complete = self.simulate_roots(
                    world,
                    random,
                    pos,
                    direction,
                    root_origin,
                    root_positions,
                    layer + 1,
                );
                if !complete {
                    return false;
                }
            }
        }
        true
    }

    /// 官方 `MangroveRootPlacer.potentialRootPositions`。
    ///
    /// Official `MangroveRootPlacer.potentialRootPositions`.
    fn potential_root_positions(
        &self,
        pos: Pos,
        direction: (i32, i32),
        random: &WorldgenRandom,
        root_origin: Pos,
    ) -> Vec<Pos> {
        let below = (pos.0, pos.1 - 1, pos.2);
        let next_to = (pos.0 + direction.0, pos.1, pos.2 + direction.1);
        let width = (pos.0 - root_origin.0).abs()
            + (pos.1 - root_origin.1).abs()
            + (pos.2 - root_origin.2).abs();
        let max_width = self.max_root_width;
        if width > max_width - 3 && width <= max_width {
            if random.next_float() < self.random_skew_chance {
                vec![below, (next_to.0, next_to.1 - 1, next_to.2)]
            } else {
                vec![below]
            }
        } else if width > max_width || random.next_float() < self.random_skew_chance {
            vec![below]
        } else if random.next_boolean() {
            vec![next_to]
        } else {
            vec![below]
        }
    }

    /// 官方 `MangroveRootPlacer.canPlaceRoot`。
    ///
    /// Official `MangroveRootPlacer.canPlaceRoot`.
    fn can_place_root(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        valid_tree_pos(world, pos)
            || in_set(&self.can_grow_through, world.get_block(pos.0, pos.1, pos.2))
    }

    /// 官方 `MangroveRootPlacer.placeRoot`：泥巴里的树根换成沾泥的树根。
    ///
    /// Official `MangroveRootPlacer.placeRoot`: roots inside mud become muddy roots.
    fn place_root(&self, place: &mut TreePlace<'_, '_>, random: &WorldgenRandom, pos: Pos) {
        if in_set(&self.muddy_roots_in, place.get_block(pos)) {
            if let Some(muddy) = self
                .muddy_roots_provider
                .get_state(place.world, random, pos)
            {
                let state = potentially_waterlogged(place, pos, muddy);
                place.set_root(pos, state);
            }
            return;
        }
        // `RootPlacer.placeRoot`。
        if !self.can_place_root(place.world, pos) {
            return;
        }
        if let Some(root) = self.root_provider.get_state(place.world, random, pos) {
            let state = potentially_waterlogged(place, pos, root);
            place.set_root(pos, state);
        }
        if let Some((provider, chance)) = &self.above_root {
            let above = (pos.0, pos.1 + 1, pos.2);
            if random.next_float() < *chance
                && is_air(place.get_block(above))
                && let Some(state) = provider.get_state(place.world, random, above)
            {
                let state = potentially_waterlogged(place, above, state);
                place.set_root(above, state);
            }
        }
    }
}

fn in_set(ids: &[i32], state: u32) -> bool {
    block_id_of_state(state).is_some_and(|id| ids.contains(&(id as i32)))
}

/// 官方 `RootPlacer.getPotentiallyWaterloggedState`：这一格的流体在 `water` 标签里
/// （水源或流动的水）就带上 `waterlogged`。
///
/// Official `RootPlacer.getPotentiallyWaterloggedState`: `waterlogged` is set when the
/// fluid at the position is in the `water` tag (source or flowing).
fn potentially_waterlogged(place: &TreePlace<'_, '_>, pos: Pos, state: u32) -> u32 {
    if !has_property(state, "waterlogged") {
        return state;
    }
    let fluid = state_fluid(place.get_block(pos)).fluid;
    let water = ["minecraft:water", "minecraft:flowing_water"]
        .iter()
        .any(|name| registries::fluid::by_name(name) == Some(fluid));
    with_property(state, "waterlogged", if water { "true" } else { "false" }).unwrap_or(state)
}
