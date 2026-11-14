//! 橡树用到的 `TreeFeature` 配置字段（26.3 的 `TreeFeature` 是 record，字段直接展开在
//! feature 对象上）。只接受 straight trunk + blob foliage + two_layers_feature_size +
//! 空 decorators + 无 root placer + `soil_beneath_tree` 这一组，其余一律返回 `None`
//! 让分发把整个 placed feature 标成 unsupported。
//!
//! The `TreeFeature` config fields used by oak (26.3 `TreeFeature` is a record whose
//! fields are inlined on the feature object). Only straight trunk + blob foliage +
//! two_layers_feature_size + empty decorators + no root placer + `soil_beneath_tree`
//! is accepted; anything else returns `None` so the placed feature stays unsupported.

use serde_json::Value;

use crate::feature::{state::parse_state, state_provider::StateProvider};

use super::{foliage::BlobFoliagePlacer, trunk::StraightTrunkPlacer};

/// 已解析、能逐块对拍的橡树配置。
///
/// A parsed oak tree config that can be compared block-for-block.
pub struct TreeConfig {
    pub trunk: StraightTrunkPlacer,
    pub foliage: BlobFoliagePlacer,
    pub trunk_state: u32,
    pub foliage_state: u32,
    /// `soil_beneath_tree` 规则命中时放的泥土状态。
    ///
    /// The dirt state placed when the `soil_beneath_tree` rule matches.
    pub below_trunk_dirt: u32,
    pub limit: i32,
    pub lower_size: i32,
    pub upper_size: i32,
    pub min_clipped_height: Option<i32>,
    pub ignore_vines: bool,
}

impl TreeConfig {
    /// 解析配置；任一未移植的 placer / decorator / provider / minimum_size 返回 `None`。
    ///
    /// Parse the config; return `None` for any unported placer, decorator, provider
    /// or minimum_size.
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        if object.get("type").and_then(Value::as_str) != Some("minecraft:tree") {
            return None;
        }
        // root placer 和 decorators 都还没移植；有就整套标 unsupported。
        // Root placers and decorators are unported; their presence marks the whole
        // placed feature unsupported.
        if object.contains_key("root_placer") {
            return None;
        }
        if !object.get("decorators")?.as_array()?.is_empty() {
            return None;
        }
        let trunk = StraightTrunkPlacer::parse(object.get("trunk_placer")?)?;
        let foliage = BlobFoliagePlacer::parse(object.get("foliage_placer")?)?;
        let trunk_state = simple_state(object.get("trunk_provider")?)?;
        let foliage_state = simple_state(object.get("foliage_provider")?)?;
        // `below_trunk_provider` 只认 `soil_beneath_tree` 这个注册表引用。
        // Only the `soil_beneath_tree` registry reference is ported.
        if object.get("below_trunk_provider")?.as_str() != Some("minecraft:soil_beneath_tree") {
            return None;
        }
        let below_trunk_dirt = parse_state(&Value::String("minecraft:dirt".to_owned())).ok()?;
        let size = object.get("minimum_size")?.as_object()?;
        if size.get("type").and_then(Value::as_str) != Some("minecraft:two_layers_feature_size") {
            return None;
        }
        let field = |name: &str, default: i32| -> i32 {
            size.get(name)
                .and_then(Value::as_i64)
                .map_or(default, |n| n as i32)
        };
        let limit = field("limit", 1);
        let lower_size = field("lower_size", 0);
        let upper_size = field("upper_size", 1);
        let min_clipped_height = size
            .get("min_clipped_height")
            .and_then(Value::as_i64)
            .map(|n| n as i32);
        let ignore_vines = object
            .get("ignore_vines")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Some(Self {
            trunk,
            foliage,
            trunk_state,
            foliage_state,
            below_trunk_dirt,
            limit,
            lower_size,
            upper_size,
            min_clipped_height,
            ignore_vines,
        })
    }

    /// `TwoLayersFeatureSize.getSizeAtHeight(treeHeight, yo)`。
    ///
    /// Official `TwoLayersFeatureSize.getSizeAtHeight(treeHeight, yo)`.
    pub fn size_at_height(&self, _tree_height: i32, yo: i32) -> i32 {
        if yo < self.limit {
            self.lower_size
        } else {
            self.upper_size
        }
    }
}

/// 只接受内联 bare state（`SimpleStateProvider`）；typed provider / holder 引用返回 `None`。
///
/// Only inline bare states (`SimpleStateProvider`) are accepted; typed providers and
/// holder references return `None`.
fn simple_state(value: &Value) -> Option<u32> {
    match StateProvider::parse(value)? {
        StateProvider::Simple(state) => Some(state),
        _ => None,
    }
}
