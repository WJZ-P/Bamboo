//! `SimpleBlockFeature`：花草/南瓜这类「放方块」的地物。
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

use mc_bamboo_data::{facts, state_facts, state_fluid};

use crate::random::WorldgenRandom;

use super::{
    state::{parse_state, with_property},
    survival,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 已解析、已确认能逐块对拍的 `SimpleBlockFeature`。
pub struct SimpleBlockFeature {
    state: u32,
}

impl SimpleBlockFeature {
    /// 只接受内联 bare state，且该方块的存活判定已移植；否则返回 `None`（标 unsupported）。
    ///
    /// Accept only an inline bare state whose survival check is ported; otherwise return
    /// `None` (unsupported).
    pub fn parse(config: &Value) -> Option<Self> {
        let to_place = config.as_object()?.get("to_place")?;
        // 有 `type` 字段的是 typed provider（weighted / randomized_int），暂不支持；
        // 字符串是 BLOCK_STATE_PROVIDER 注册表引用，未导出，也不支持。
        // A `type` field means a typed provider; a string is a holder reference.
        if to_place.get("type").is_some() || to_place.is_string() {
            return None;
        }
        let state = parse_state(to_place).ok()?;
        if !survival::supports(state) {
            return None;
        }
        Some(Self { state })
    }

    /// 官方 `SimpleBlockFeature.place` 的单个候选位置。SimpleStateProvider.getState
    /// 不消费随机数，所以这里只读 `random` 是为了和递归/组合类共用签名。
    ///
    /// One candidate of official `SimpleBlockFeature.place`. `SimpleStateProvider.getState`
    /// consumes no random; `random` is kept for a uniform signature with composition features.
    pub fn place(&self, world: &mut FeatureWorld<'_>, _random: &WorldgenRandom, pos: Pos) {
        if !survival::can_survive(self.state, world, pos) {
            return;
        }
        if survival::is_double_plant(self.state) {
            self.place_at(world, pos);
        } else {
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, self.state);
        }
    }

    /// `DoublePlantBlock.placeAt(level, state, lowerPos, 2)`：先照官方检查上一格能否
    /// 容纳上半部分，再放下 `half=lower` / `half=upper` 两格。26.3 导出的三种双格植物
    /// （tall_grass / sunflower / large_fern）只有 `half` 属性，没有 `waterlogged`，
    /// 所以 `copyWaterloggedFrom` 在这里是 no-op。
    ///
    /// `DoublePlantBlock.placeAt`: check the block above, then place `half=lower` / `half=upper`.
    /// The three exported double plants have only a `half` property (no `waterlogged`), so
    /// `copyWaterloggedFrom` is a no-op here.
    fn place_at(&self, world: &mut FeatureWorld<'_>, pos: Pos) {
        let (x, y, z) = pos;
        let above = world.get_block(x, y + 1, z);
        let above_air = state_facts(above) & facts::AIR != 0;
        if !above_air {
            // `!aboveState.isAir() && (!fluidEquals || !aboveState.canBeReplaced())` 时不放。
            // Skip when the block above is neither air nor (fluid-equal and replaceable).
            let fluid_same = state_fluid(self.state).fluid == state_fluid(above).fluid;
            let replaceable = state_facts(above) & facts::REPLACEABLE != 0;
            if !fluid_same || !replaceable {
                return;
            }
        }
        let lower = with_property(self.state, "half", "lower").unwrap_or(self.state);
        let upper = with_property(self.state, "half", "upper").unwrap_or(self.state);
        world.set_block_with_heightmaps(x, y, z, lower);
        world.set_block_with_heightmaps(x, y + 1, z, upper);
    }
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
    fn double_plants_are_supported_but_special_survival_is_gated() {
        // 双格植物现在走 placeAt，已支持。
        // Double plants now go through placeAt and are supported.
        let tall: Value = json!({"to_place": {"id": "minecraft:tall_grass"}});
        assert!(SimpleBlockFeature::parse(&tall).is_some());
        // 蘑菇要亮度检查，仍未移植。
        // Mushrooms need a brightness check; still unported.
        let mushroom: Value = json!({"to_place": {"id": "minecraft:brown_mushroom"}});
        assert!(SimpleBlockFeature::parse(&mushroom).is_none());
    }
}
