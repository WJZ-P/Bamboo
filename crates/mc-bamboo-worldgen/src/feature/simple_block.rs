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

use super::{state::with_property, state_provider::StateProvider, survival, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// 已解析、已确认能逐块对拍的 `SimpleBlockFeature`。
pub struct SimpleBlockFeature {
    provider: StateProvider,
}

impl SimpleBlockFeature {
    /// 解析 `to_place` 为已移植的 provider，且所有可能状态的存活判定都已移植；
    /// 未移植的 typed provider 返回 `None`（标 unsupported）。字符串 holder 引用按
    /// `lookup` 查 `block_state_provider` 注册表。
    ///
    /// Parse `to_place` into a ported provider whose every possible state has a ported
    /// survival check; unported typed providers return `None`. String holder references
    /// resolve through `lookup` against the `block_state_provider` registry.
    pub fn parse<'a>(
        config: &'a Value,
        lookup: &dyn Fn(&str) -> Option<&'a Value>,
    ) -> Option<Self> {
        let to_place = config.as_object()?.get("to_place")?;
        let provider = StateProvider::parse_with(to_place, lookup)?;
        if !provider
            .possible_states()
            .iter()
            .all(|&state| survival::supports(state))
        {
            return None;
        }
        Some(Self { provider })
    }

    /// 官方 `SimpleBlockFeature.place` 的单个候选位置：`getOptionalState`（typed provider
    /// 会消费随机数）→ `canSurvive` → 按方块类分派放置。
    ///
    /// One candidate of official `SimpleBlockFeature.place`: `getOptionalState` (typed
    /// providers consume random), then `canSurvive`, then dispatch by block class.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        region_random: &WorldgenRandom,
        pos: Pos,
    ) -> bool {
        let Some(state) = self.provider.get_state(random) else {
            return false;
        };
        if !survival::can_survive(state, world, pos) {
            return false;
        }
        if survival::is_double_plant(state) {
            self.place_at(world, pos, state)
        } else if let Some(carpet) = super::mossy_carpet::MossyCarpet::parse(state) {
            // `stateToPlace.getBlock() instanceof MossyCarpetBlock` →
            // `MossyCarpetBlock.placeAt(level, origin, level.getRandom(), 2)`。
            // `MossyCarpetBlock` goes through `placeAt` with `level.getRandom()`.
            carpet.place_at(world, region_random, pos)
        } else {
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
            true
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
    fn place_at(&self, world: &mut FeatureWorld<'_>, pos: Pos, state: u32) -> bool {
        let (x, y, z) = pos;
        let above = world.get_block(x, y + 1, z);
        let above_air = state_facts(above) & facts::AIR != 0;
        if !above_air {
            // `!aboveState.isAir() && (!fluidEquals || !aboveState.canBeReplaced())` 时不放。
            // Skip when the block above is neither air nor (fluid-equal and replaceable).
            let fluid_same = state_fluid(state).fluid == state_fluid(above).fluid;
            let replaceable = state_facts(above) & facts::REPLACEABLE != 0;
            if !fluid_same || !replaceable {
                return false;
            }
        }
        let lower = with_property(state, "half", "lower").unwrap_or(state);
        let upper = with_property(state, "half", "upper").unwrap_or(state);
        world.set_block_with_heightmaps(x, y, z, lower);
        world.set_block_with_heightmaps(x, y + 1, z, upper);
        true
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
        assert!(SimpleBlockFeature::parse(&pumpkin, &|_| None).is_some());
        // weighted typed provider 现在支持。
        // The weighted typed provider is now supported.
        let weighted: Value = json!({
            "to_place": {"type": "minecraft:weighted", "entries": [{"data": "minecraft:poppy", "weight": 1}]},
            "type": "minecraft:simple_block",
        });
        assert!(SimpleBlockFeature::parse(&weighted, &|_| None).is_some());
        // holder 字符串引用未导出的注册表，也不支持。
        // A holder string references an unexported registry; unsupported.
        let holder: Value =
            json!({"to_place": "minecraft:flower_plain", "type": "minecraft:simple_block"});
        assert!(SimpleBlockFeature::parse(&holder, &|_| None).is_none());
    }

    #[test]
    fn double_plants_are_supported_but_special_survival_is_gated() {
        // 双格植物现在走 placeAt，已支持。
        // Double plants now go through placeAt and are supported.
        let tall: Value = json!({"to_place": {"id": "minecraft:tall_grass"}});
        assert!(SimpleBlockFeature::parse(&tall, &|_| None).is_some());
        // 蘑菇的亮度判据在 FEATURES 阶段恒为 15，等价于只看下面那格的 override 标签，
        // 所以现在也支持。
        // The mushroom's brightness is always 15 during FEATURES, which reduces the check
        // to the override tag below, so it is supported now.
        let mushroom: Value = json!({"to_place": {"id": "minecraft:brown_mushroom"}});
        assert!(SimpleBlockFeature::parse(&mushroom, &|_| None).is_some());
    }
}
