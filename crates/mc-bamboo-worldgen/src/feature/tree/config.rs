//! `TreeFeature` 的配置（26.3 的 `TreeFeature` 是 record，字段直接展开在 feature
//! 对象上）。任何一处用到还没移植的东西，[`TreeConfig::parse`] 就返回 `None`，让分发
//! 把整个 placed feature 标成 unsupported。
//!
//! The `TreeFeature` config (26.3 `TreeFeature` is a record whose fields are inlined on
//! the feature object). [`TreeConfig::parse`] returns `None` as soon as anything unported
//! is used, so the placed feature stays unsupported.

use serde_json::Value;

use crate::{
    feature::{state::parse_state, state_provider::StateProvider},
    random::WorldgenRandom,
};

use super::{
    Pos, TreePlace, cannot_replace_below_trunk, foliage::FoliagePlacer, trunk::TrunkPlacer,
};

/// 一棵树的全部已解析配置。
///
/// The full parsed config of one tree.
pub struct TreeConfig {
    pub trunk: TrunkPlacer,
    pub foliage: FoliagePlacer,
    pub trunk_provider: StateProvider,
    pub foliage_provider: StateProvider,
    pub below_trunk: BelowTrunk,
    pub size: FeatureSize,
    pub ignore_vines: bool,
}

impl TreeConfig {
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        if object.get("type").and_then(Value::as_str) != Some("minecraft:tree") {
            return None;
        }
        // root placer 和 decorators 还没移植。
        // Root placers and decorators are unported.
        if object.contains_key("root_placer") {
            return None;
        }
        if !object.get("decorators")?.as_array()?.is_empty() {
            return None;
        }
        Some(Self {
            trunk: TrunkPlacer::parse(object.get("trunk_placer")?)?,
            foliage: FoliagePlacer::parse(object.get("foliage_placer")?)?,
            trunk_provider: StateProvider::parse(object.get("trunk_provider")?)?,
            foliage_provider: StateProvider::parse(object.get("foliage_provider")?)?,
            below_trunk: BelowTrunk::parse(object.get("below_trunk_provider")?)?,
            size: FeatureSize::parse(object.get("minimum_size")?)?,
            ignore_vines: object
                .get("ignore_vines")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// `below_trunk_provider`：树干正下方那一格放什么。
///
/// `below_trunk_provider`: what goes directly below the trunk.
pub enum BelowTrunk {
    /// 注册表引用 `minecraft:soil_beneath_tree`：一条规则的 `RuleBasedStateProvider`，
    /// 当前方块不在 `cannot_replace_below_tree_trunk` 标签里就给泥土，否则不放。
    ///
    /// The registry reference `minecraft:soil_beneath_tree`: a one-rule
    /// `RuleBasedStateProvider` giving dirt unless the current block is in the
    /// `cannot_replace_below_tree_trunk` tag, in which case nothing is placed.
    SoilBeneathTree { dirt: u32 },
    /// 内联的 `BlockStateProvider`，总是给出状态。
    ///
    /// An inline `BlockStateProvider`, which always yields a state.
    Provider(StateProvider),
}

impl BelowTrunk {
    fn parse(value: &Value) -> Option<Self> {
        match value {
            Value::String(name) if name == "minecraft:soil_beneath_tree" => {
                let dirt = parse_state(&Value::String("minecraft:dirt".to_owned())).ok()?;
                Some(Self::SoilBeneathTree { dirt })
            }
            Value::String(_) => None,
            _ => Some(Self::Provider(StateProvider::parse(value)?)),
        }
    }

    /// 官方 `TrunkPlacer.placeBelowTrunkBlock`。
    ///
    /// Official `TrunkPlacer.placeBelowTrunkBlock`.
    pub(crate) fn place(&self, place: &mut TreePlace<'_, '_>, random: &WorldgenRandom, pos: Pos) {
        let state = match self {
            Self::SoilBeneathTree { dirt } => {
                if cannot_replace_below_trunk(place.get_block(pos)) {
                    return;
                }
                *dirt
            }
            Self::Provider(provider) => match provider.get_state(random) {
                Some(state) => state,
                None => return,
            },
        };
        place.set_trunk(pos, state);
    }
}

/// `FeatureSize`：`two_layers_feature_size` 和 `three_layers_feature_size`。
///
/// `FeatureSize`: `two_layers_feature_size` and `three_layers_feature_size`.
pub struct FeatureSize {
    pub min_clipped_height: Option<i32>,
    limit: i32,
    lower_size: i32,
    upper_size: i32,
    /// 只有三层的有：`(upper_limit, middle_size)`。
    ///
    /// Three-layer sizes only: `(upper_limit, middle_size)`.
    middle: Option<(i32, i32)>,
}

impl FeatureSize {
    fn parse(value: &Value) -> Option<Self> {
        let size = value.as_object()?;
        let field = |name: &str, default: i32| -> i32 {
            size.get(name)
                .and_then(Value::as_i64)
                .map_or(default, |n| n as i32)
        };
        let middle = match size.get("type")?.as_str()? {
            "minecraft:two_layers_feature_size" => None,
            "minecraft:three_layers_feature_size" => {
                Some((field("upper_limit", 1), field("middle_size", 1)))
            }
            _ => return None,
        };
        Some(Self {
            min_clipped_height: size
                .get("min_clipped_height")
                .and_then(Value::as_i64)
                .map(|n| n as i32),
            limit: field("limit", 1),
            lower_size: field("lower_size", 0),
            upper_size: field("upper_size", 1),
            middle,
        })
    }

    /// 官方 `getSizeAtHeight(treeHeight, yo)`。
    ///
    /// Official `getSizeAtHeight(treeHeight, yo)`.
    pub fn size_at_height(&self, tree_height: i32, yo: i32) -> i32 {
        if yo < self.limit {
            return self.lower_size;
        }
        match self.middle {
            Some((upper_limit, middle_size)) if yo < tree_height - upper_limit => middle_size,
            _ => self.upper_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_trees_and_gates_unported_ones() {
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let features = root["feature"].as_object().unwrap();
        // 没有装饰器和树根的树都能解析。
        // Trees without decorators or roots parse.
        for name in ["minecraft:oak", "minecraft:fancy_oak", "minecraft:acacia"] {
            assert!(TreeConfig::parse(&features[name]).is_some(), "{name}");
        }
        // swamp_oak 带 leave_vine 装饰器。
        // swamp_oak has a leave_vine decorator.
        assert!(TreeConfig::parse(&features["minecraft:swamp_oak"]).is_none());
    }

    #[test]
    fn three_layer_size_uses_the_middle_band() {
        let size = FeatureSize {
            min_clipped_height: None,
            limit: 1,
            lower_size: 0,
            upper_size: 2,
            middle: Some((1, 1)),
        };
        assert_eq!(size.size_at_height(6, 0), 0);
        assert_eq!(size.size_at_height(6, 3), 1);
        assert_eq!(size.size_at_height(6, 5), 2);
    }
}
