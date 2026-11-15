//! `SimpleBlockFeature`：花草/南瓜这类「放方块」的地物。
//!
//! 官方 `SimpleBlockFeature.place`：`to_place.getOptionalState` → `canSurvive` →
//! 按方块类型分派（DoublePlantBlock / MossyCarpetBlock 走 placeAt，其余 `setBlock(pos, state, 2)`）。
//! 26.3 的 `to_place` 是 `Holder<BlockStateProvider>`：内联 bare state（`{"id":...}`）等价于
//! `SimpleStateProvider`，注册名字符串按 `block_state_provider` 注册表解析，
//! `weighted` / `randomized_int` / `random_block` 是 typed provider。
//!
//! Official `SimpleBlockFeature.place`: `getOptionalState` -> `canSurvive` -> dispatch by block
//! class (DoublePlantBlock / MossyCarpetBlock use placeAt, else `setBlock(pos, state, 2)`).
//! `to_place` is a `Holder<BlockStateProvider>`: an inline bare state is a
//! `SimpleStateProvider`, a registry name resolves through `block_state_provider`, and
//! `weighted` / `randomized_int` / `random_block` are typed providers.

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
        if !provider.supported() {
            return None;
        }
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
        region_random: &crate::random::Random,
        pos: Pos,
    ) -> bool {
        let Some(state) = self.provider.get_optional_state(world, random, pos) else {
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
    /// 容纳上半部分，再放下 `half=lower` / `half=upper` 两格。两格各自
    /// `copyWaterloggedFrom`：状态有 `waterlogged` 属性时按该格是否有水设置，
    /// `small_dripleaf` 就是这种情况；`tall_grass` / `sunflower` / `large_fern` 没有
    /// 这个属性，那一步是 no-op。
    ///
    /// `DoublePlantBlock.placeAt`: check the block above, then place `half=lower` /
    /// `half=upper`. Each half runs `copyWaterloggedFrom`: a state carrying
    /// `waterlogged` is set from whether that cell holds water (`small_dripleaf` does),
    /// while `tall_grass` / `sunflower` / `large_fern` have no such property and skip it.
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
        let lower = copy_waterlogged_from(world, (x, y, z), lower);
        let upper = copy_waterlogged_from(world, (x, y + 1, z), upper);
        world.set_block_with_heightmaps(x, y, z, lower);
        world.set_block_with_heightmaps(x, y + 1, z, upper);
        true
    }
}

/// `DoublePlantBlock.copyWaterloggedFrom(level, pos, state)`：状态有 `waterlogged`
/// 属性时，按该格 `getFluidState(pos).is(FluidTags.WATER)` 设置它。
///
/// `DoublePlantBlock.copyWaterloggedFrom(level, pos, state)`: when the state carries a
/// `waterlogged` property, set it from `getFluidState(pos).is(FluidTags.WATER)`.
fn copy_waterlogged_from(world: &FeatureWorld<'_>, pos: Pos, state: u32) -> u32 {
    let water = in_water_tag(world.get_block(pos.0, pos.1, pos.2));
    if water {
        with_property(state, "waterlogged", "true").unwrap_or(state)
    } else {
        with_property(state, "waterlogged", "false").unwrap_or(state)
    }
}

/// `FluidState.is(FluidTags.WATER)`。
///
/// `FluidState.is(FluidTags.WATER)`.
fn in_water_tag(state: u32) -> bool {
    let fluid = state_fluid(state).fluid;
    mc_bamboo_data::tags::TAG_GROUPS[mc_bamboo_data::tags::fluid_tags::GROUP]
        .tags
        .iter()
        .find(|entry| entry.name == "minecraft:water")
        .is_some_and(|entry| entry.ids.contains(&fluid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 从导出的 `worldgen.json` 取 `block_state_provider` 注册表，供 holder 字符串解析。
    ///
    /// The `block_state_provider` registry from the exported `worldgen.json`, so holder
    /// strings resolve the same way the real dispatch does.
    fn parse_with_registry(config: &Value) -> Option<SimpleBlockFeature> {
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let providers = &root["block_state_provider"];
        SimpleBlockFeature::parse(config, &|name| providers.get(name))
    }

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
        // holder 字符串通过 `lookup` 查 `block_state_provider` 注册表，现在支持。
        // Holder strings resolve through `lookup` against the registry; supported now.
        let holder: Value =
            json!({"to_place": "minecraft:flower_plain", "type": "minecraft:simple_block"});
        assert!(parse_with_registry(&holder).is_some());
        // rule_based provider 的 soil_beneath_tree 现在支持。
        // The rule_based soil_beneath_tree provider is supported.
        let supported: Value =
            json!({"to_place": "minecraft:soil_beneath_tree", "type": "minecraft:simple_block"});
        assert!(parse_with_registry(&supported).is_some());
        // rotated provider 现在也移植了（珊瑚链依赖它）。
        // The rotated provider is ported now (the coral chain needs it).
        let rotated: Value = json!({
            "to_place": {
                "type": "minecraft:rotated",
                "state": {"id": "minecraft:basalt", "properties": {"axis": "y"}},
            },
            "type": "minecraft:simple_block",
        });
        assert!(SimpleBlockFeature::parse(&rotated, &|_| None).is_some());
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

    #[test]
    fn parses_rule_based_holder_and_rejects_unported_predicates() {
        // soil_beneath_tree 是 rule_based provider，谓词全部已移植。
        // soil_beneath_tree is a rule_based provider whose predicate is ported.
        let soil: Value =
            json!({"to_place": "minecraft:soil_beneath_tree", "type": "minecraft:simple_block"});
        assert!(parse_with_registry(&soil).is_some());
        // rule_based provider 里的 podzol 现在也能解析（它的方块类是 SnowyBlock，默认存活）。
        // The rule_based podzol provider now parses too (SnowyBlock, default survival).
        let podzol: Value =
            json!({"to_place": "minecraft:podzol_beneath_tree", "type": "minecraft:simple_block"});
        assert!(parse_with_registry(&podzol).is_some());
    }

    #[test]
    fn parses_water_plant_holders() {
        // 睡莲、海泡菜、孢子花、海草的存活判定已移植，这些配置现在能解析。
        // Lily pad, sea pickle, spore blossom and seagrass survival checks are ported, so
        // these configs parse.
        let lily: Value = json!({"to_place": {"id": "minecraft:lily_pad"}});
        assert!(SimpleBlockFeature::parse(&lily, &|_| None).is_some());
        // 海泡菜用内联 randomized_int provider（pickles 1..4）。
        // Sea pickles use an inline randomized_int provider (pickles 1..4).
        let pickle: Value = json!({
            "to_place": {
                "property": "pickles",
                "source": {"id": "minecraft:sea_pickle", "properties": {"pickles": "1", "waterlogged": "true"}},
                "type": "minecraft:randomized_int",
                "values": {"min_inclusive": 1, "max_inclusive": 4, "type": "minecraft:uniform"},
            },
        });
        assert!(SimpleBlockFeature::parse(&pickle, &|_| None).is_some());
        let blossom: Value = json!({"to_place": {"id": "minecraft:spore_blossom"}});
        assert!(SimpleBlockFeature::parse(&blossom, &|_| None).is_some());
        let seagrass: Value = json!({"to_place": {"id": "minecraft:seagrass"}});
        assert!(SimpleBlockFeature::parse(&seagrass, &|_| None).is_some());
        let tall: Value = json!({
            "to_place": {"id": "minecraft:tall_seagrass", "properties": {"half": "lower"}},
        });
        assert!(SimpleBlockFeature::parse(&tall, &|_| None).is_some());
    }
}
