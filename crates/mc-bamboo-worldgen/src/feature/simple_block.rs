//! `SimpleBlockFeature`：花草/南瓜这类「放一个方块」的地物。
//!
//! 官方 `SimpleBlockFeature.place`：`to_place.getOptionalState` → `canSurvive` →
//! 按方块类型分派（DoublePlantBlock / MossyCarpetBlock 走 placeAt，其余 `setBlock(pos, state, 2)`）。
//! 26.3 的 `to_place` 是 `Holder<BlockStateProvider>`：内联 bare state（`{"id":...}`）等价于
//! `SimpleStateProvider`，注册名字符串引用的是 `BLOCK_STATE_PROVIDER` 注册表（本轮未导出），
//! `weighted` / `randomized_int` 是 typed provider。这里只实现内联 bare state，其余标 unsupported。
//!
//! Official `SimpleBlockFeature.place`: `getOptionalState` -> `canSurvive` -> dispatch by block
//! class (DoublePlantBlock / MossyCarpetBlock use placeAt, else `setBlock(pos, state, 2)`).
//! Only inline bare-state `to_place` (SimpleStateProvider) is ported; holder references and
//! typed providers stay unsupported.

use serde_json::Value;

use mc_bamboo_data::{BlockClass, block_id_of_state, block_of_state, block_tag};

use crate::random::WorldgenRandom;

use super::{state::parse_state, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// `canSurvive` 的形态：默认 true，或检查下面方块在哪个官方标签里。
///
/// `canSurvive` shape: default true, or a block-tag check on the block below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Survival {
    Default,
    Vegetation(&'static str),
}

/// 已解析、已确认能逐块对拍的 `SimpleBlockFeature`。
pub struct SimpleBlockFeature {
    state: u32,
    survival: Survival,
}

impl SimpleBlockFeature {
    /// 只接受内联 bare state；解析不了或存活判定未移植时返回 `None`（标 unsupported）。
    ///
    /// Accept only an inline bare state; return `None` (unsupported) otherwise.
    pub fn parse(config: &Value) -> Option<Self> {
        let to_place = config.as_object()?.get("to_place")?;
        // 有 `type` 字段的是 typed provider（weighted / randomized_int），暂不支持；
        // 字符串是 BLOCK_STATE_PROVIDER 注册表引用，未导出，也不支持。
        // A `type` field means a typed provider; a string is a holder reference.
        if to_place.get("type").is_some() || to_place.is_string() {
            return None;
        }
        let state = parse_state(to_place).ok()?;
        let survival = classify(state)?;
        Some(Self { state, survival })
    }

    /// 官方 `SimpleBlockFeature.place` 的单个候选位置。SimpleStateProvider.getState
    /// 不消费随机数，所以这里只读 `random` 是为了和递归/组合类共用签名。
    ///
    /// One candidate of official `SimpleBlockFeature.place`. `SimpleStateProvider.getState`
    /// consumes no random; `random` is kept for a uniform signature with composition features.
    pub fn place(&self, world: &mut FeatureWorld<'_>, _random: &WorldgenRandom, pos: Pos) {
        if !self.can_survive(world, pos) {
            return;
        }
        // 只放行白名单里的类，DoublePlant/MossyCarpet 的 placeAt 分支不会走到。
        // Only whitelisted classes reach here; no DoublePlant/MossyCarpet placeAt branch.
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, self.state);
    }

    /// `VegetationBlock.canSurvive` / `DryVegetationBlock.mayPlaceOn`：下面那格
    /// 是否 `is(supports_vegetation)` / `is(supports_dry_vegetation)`；其余类默认 true。
    ///
    /// `VegetationBlock.canSurvive` / `DryVegetationBlock.mayPlaceOn`: whether the block
    /// below is in the tag; plain blocks default to true.
    fn can_survive(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        match self.survival {
            Survival::Default => true,
            Survival::Vegetation(tag) => {
                let below = world.get_block(pos.0, pos.1 - 1, pos.2);
                let id = block_id_of_state(below).map_or(-1, |id| id as i32);
                block_tag(tag).is_some_and(|ids| ids.contains(&id))
            }
        }
    }
}

/// 26.3 里 BushBlock / FlowerBlock / TallGrassBlock / DryVegetationBlock 都直接继承
/// `VegetationBlock`，`canSurvive` 就是标签判定；PumpkinBlock 和纯 Block 用默认 true。
/// 其它类（蘑菇要亮度、DoublePlant 要上下两格 placeAt、MossyCarpet/Fire/LilyPad/
/// SporeBlossom/Nether 植物各有特殊存活）一律不在这里放行，避免静默算错。
///
/// In 26.3 these four classes extend `VegetationBlock` directly, so `canSurvive` is a tag
/// check; PumpkinBlock and plain Block default to true. Every other class is gated out.
fn classify(state: u32) -> Option<Survival> {
    let class = block_of_state(state)?.class;
    Some(match class {
        BlockClass::PumpkinBlock | BlockClass::Block => Survival::Default,
        BlockClass::BushBlock | BlockClass::FlowerBlock | BlockClass::TallGrassBlock => {
            Survival::Vegetation("minecraft:supports_vegetation")
        }
        BlockClass::DryVegetationBlock => Survival::Vegetation("minecraft:supports_dry_vegetation"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_inline_bare_state_and_gates_unported_forms() {
        let pumpkin: Value =
            json!({"to_place": {"id": "minecraft:pumpkin"}, "type": "minecraft:simple_block"});
        assert!(SimpleBlockFeature::parse(&pumpkin).is_some());
        // typed provider（weighted）暂不支持。
        // A typed provider (weighted) is not supported yet.
        let weighted: Value = json!({
            "to_place": {"type": "minecraft:weighted", "entries": [{"data": "minecraft:poppy", "weight": 1}]},
            "type": "minecraft:simple_block",
        });
        assert!(SimpleBlockFeature::parse(&weighted).is_none());
        // holder 字符串引用未导出的注册表，也不支持。
        // A holder string references an unexported registry; unsupported.
        let holder: Value =
            json!({"to_place": "minecraft:flower_plain", "type": "minecraft:simple_block"});
        assert!(SimpleBlockFeature::parse(&holder).is_none());
    }

    #[test]
    fn gates_classes_with_special_survival() {
        // 蘑菇要亮度检查，DoublePlant 要 placeAt，都不在白名单里。
        // Mushrooms need brightness, DoublePlant needs placeAt; both gated out.
        let mushroom: Value = json!({"to_place": {"id": "minecraft:brown_mushroom"}});
        assert!(SimpleBlockFeature::parse(&mushroom).is_none());
        let tall: Value =
            json!({"to_place": {"id": "minecraft:tall_grass", "properties": {"half": "lower"}}});
        assert!(SimpleBlockFeature::parse(&tall).is_none());
    }

    #[test]
    fn classifies_vegetation_and_default() {
        assert_eq!(
            classify(
                mc_bamboo_data::block_by_name("minecraft:pumpkin")
                    .unwrap()
                    .default_state
            ),
            Some(Survival::Default)
        );
        assert_eq!(
            classify(
                mc_bamboo_data::block_by_name("minecraft:short_grass")
                    .unwrap()
                    .default_state
            ),
            Some(Survival::Vegetation("minecraft:supports_vegetation"))
        );
        assert_eq!(
            classify(
                mc_bamboo_data::block_by_name("minecraft:dead_bush")
                    .unwrap()
                    .default_state
            ),
            Some(Survival::Vegetation("minecraft:supports_dry_vegetation"))
        );
    }
}
