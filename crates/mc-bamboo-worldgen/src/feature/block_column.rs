//! `BlockColumnFeature`：沿一个方向逐层叠放方块。
//!
//! 官方先为每层采样高度（消费随机数），再从 `origin + direction` 开始按
//! `allowed_placement` 检查一路的空间，放不下就 `truncate` 砍掉尾部（或头部）。
//! 放置阶段每放一格都调一次 `BlockStateProvider.getState`（可能再消费随机数），
//! 走 `setBlock(pos, state, 2)`，即 `ChunkAccess.setBlockState` 更新高度图。
//!
//! `BlockColumnFeature`: stack blocks layer by layer along one direction. The
//! official code samples each layer's height first (consuming random), then checks
//! `allowed_placement` from `origin + direction` forward, truncating the tail (or
//! head) when a cell fails. Each placed block calls `BlockStateProvider.getState`
//! (may consume more random) and goes through `setBlock(pos, state, 2)`.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate, provider::IntProvider, state_provider::StateProvider,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// `BlockColumnFeature.Layer`：一个高度提供者加一个状态提供者。
///
/// `BlockColumnFeature.Layer`: a height provider and a state provider.
struct Layer {
    height: IntProvider,
    provider: StateProvider,
}

/// 一个解析好的 `minecraft:block_column`。
///
/// A parsed `minecraft:block_column`.
pub struct BlockColumnFeature {
    layers: Vec<Layer>,
    direction: [i32; 3],
    allowed_placement: BlockPredicate,
    prioritize_tip: bool,
}

impl BlockColumnFeature {
    /// 按官方 codec 解析；任一 provider 是注册表引用、或 `allowed_placement`
    /// 含未移植谓词时返回 `None`，让 placed feature 标 unsupported。
    ///
    /// Parse by the official codec; return `None` when a provider is a registry
    /// reference or `allowed_placement` contains an unported predicate.
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let layers = object
            .get("layers")?
            .as_array()?
            .iter()
            .map(|entry| {
                Some(Layer {
                    height: IntProvider::parse(entry.get("height")?).ok()?,
                    provider: StateProvider::parse(entry.get("provider")?)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let direction = parse_direction(object.get("direction")?.as_str()?)?;
        let allowed_placement = BlockPredicate::parse(object.get("allowed_placement")?);
        if !allowed_placement.is_supported() {
            return None;
        }
        let prioritize_tip = object
            .get("prioritize_tip")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Some(Self {
            layers,
            direction,
            allowed_placement,
            prioritize_tip,
        })
    }

    /// `BlockColumnFeature.place`，返回官方 `placedAny`（`totalHeight != 0` 即 true）。
    ///
    /// `BlockColumnFeature.place`, returns official `placedAny` (true when
    /// `totalHeight != 0`).
    pub fn place(&self, world: &mut FeatureWorld<'_>, random: &WorldgenRandom, pos: Pos) -> bool {
        let layer_count = self.layers.len();
        let mut layer_heights = vec![0i32; layer_count];
        let mut total_height = 0i32;
        for (index, layer) in self.layers.iter().enumerate() {
            layer_heights[index] = layer.height.sample(random);
            total_height += layer_heights[index];
        }
        if total_height == 0 {
            return false;
        }

        let [dx, dy, dz] = self.direction;
        // `nextPos = placePos.mutable().move(direction)`，检查从 origin 沿方向一格开始。
        // `nextPos = placePos.mutable().move(direction)`; checks start one step along.
        let mut next = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
        for y in 0..total_height {
            if !self.allowed_placement.test(world, next) {
                truncate(&mut layer_heights, total_height, y, self.prioritize_tip);
                break;
            }
            next = (next.0 + dx, next.1 + dy, next.2 + dz);
        }

        let mut place_pos = pos;
        for (index, layer) in self.layers.iter().enumerate() {
            let count = layer_heights[index];
            if count == 0 {
                continue;
            }
            for _ in 0..count {
                let Some(state) = layer.provider.get_state(random) else {
                    continue;
                };
                world.set_block_with_heightmaps(place_pos.0, place_pos.1, place_pos.2, state);
                place_pos = (place_pos.0 + dx, place_pos.1 + dy, place_pos.2 + dz);
            }
        }
        true
    }
}

/// `Direction.CODEC` 的名字到单位步长偏移。
///
/// `Direction.CODEC` name to unit-step offset.
fn parse_direction(name: &str) -> Option<[i32; 3]> {
    Some(match name {
        "up" => [0, 1, 0],
        "down" => [0, -1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        _ => return None,
    })
}

/// `BlockColumnFeature.truncate`：把总高度砍到 `new_height`。`prioritize_tip`
/// 为真时从头（第 0 层）开始删，保留末端；否则从末层开始删，保留头部。
///
/// `BlockColumnFeature.truncate`: cut total height down to `new_height`. With
/// `prioritize_tip`, remove from the first layer (keep the tip); otherwise remove
/// from the last layer (keep the base).
fn truncate(layer_heights: &mut [i32], total_height: i32, new_height: i32, prioritize_tip: bool) {
    let mut amount_to_remove = total_height - new_height;
    let direction = if prioritize_tip { 1 } else { -1 };
    let end = if prioritize_tip {
        layer_heights.len() as i32
    } else {
        -1
    };
    let mut i = if prioritize_tip {
        0
    } else {
        layer_heights.len() as i32 - 1
    };
    while i != end && amount_to_remove > 0 {
        let this_layer = layer_heights[i as usize];
        let to_remove = this_layer.min(amount_to_remove);
        amount_to_remove -= to_remove;
        layer_heights[i as usize] -= to_remove;
        i += direction;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_the_cactus_config() {
        let config = json!({
            "allowed_placement": {"tag": "minecraft:air", "type": "minecraft:matching_block_tag"},
            "direction": "up",
            "layers": [
                {"height": {"max_inclusive": 3, "min_inclusive": 1, "type": "minecraft:biased_to_bottom"},
                 "provider": {"id": "minecraft:cactus", "properties": {"age": "0"}}},
                {"height": {"distribution": [{"data": 0, "weight": 3}, {"data": 1, "weight": 1}], "type": "minecraft:weighted_list"},
                 "provider": {"id": "minecraft:cactus_flower"}}
            ],
            "prioritize_tip": false,
            "type": "minecraft:block_column",
        });
        let feature = BlockColumnFeature::parse(&config).unwrap();
        assert_eq!(feature.layers.len(), 2);
        assert_eq!(feature.direction, [0, 1, 0]);
        assert!(!feature.prioritize_tip);
    }

    #[test]
    fn holder_string_provider_is_unsupported() {
        let config = json!({
            "allowed_placement": {"tag": "minecraft:air", "type": "minecraft:matching_block_tag"},
            "direction": "down",
            "layers": [{"height": 1, "provider": "minecraft:cave_vines_head"}],
            "prioritize_tip": true,
            "type": "minecraft:block_column",
        });
        assert!(BlockColumnFeature::parse(&config).is_none());
    }

    #[test]
    fn truncate_keeps_base_or_tip() {
        // 两层高度 [3, 1]，总 4，砍到 2：不 prioritize 从末层删 → [3? no].
        // Two layers [3,1], total 4, cut to 2: without tip priority remove from
        // the last layer first.
        let mut base = vec![3, 1];
        truncate(&mut base, 4, 2, false);
        assert_eq!(base, vec![2, 0]);
        let mut tip = vec![3, 1];
        truncate(&mut tip, 4, 2, true);
        assert_eq!(tip, vec![1, 1]);
    }
}
