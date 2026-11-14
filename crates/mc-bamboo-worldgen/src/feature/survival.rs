//! 地物放置时的 `BlockState.canSurvive(level, pos)` 判定。
//!
//! `SimpleBlockFeature.place` 和 `WouldSurvivePredicate.test` 都要问「某个方块状态在
//! 这个位置能否存活」，共用这里按官方类名分派的实现。26.3 里一批植物类直接继承
//! `VegetationBlock`，`canSurvive` 就是「下面那格在某个方块标签里」；`BlockBehaviour`
//! 的默认实现返回 true。覆写了 `canSurvive` 的类（蘑菇要亮度、睡莲要流体、孢子花要
//! `canSupportCenter` 等）本轮明确不支持，调用方据此把整个 placed feature 标 unsupported。
//!
//! Shared `BlockState.canSurvive(level, pos)` for feature placement. `SimpleBlockFeature.place`
//! and `WouldSurvivePredicate.test` both dispatch on the official block class here. In 26.3
//! many plant classes extend `VegetationBlock` directly, so `canSurvive` is a tag check on the
//! block below; `BlockBehaviour`'s default returns true. Classes that override `canSurvive`
//! (mushrooms need brightness, lily pads need a fluid, spore blossoms need `canSupportCenter`)
//! stay unported and make the whole placed feature unsupported.

use mc_bamboo_data::{BlockClass, block_id_of_state, block_of_state, block_tag};

use super::world::FeatureWorld;

type Pos = (i32, i32, i32);

/// 存活判定的形状。
///
/// The shape of a survival check.
enum Kind {
    /// `BlockBehaviour` 默认：恒 true。
    Default,
    /// `VegetationBlock.canSurvive`：下面那格在给定方块标签里。
    Vegetation(&'static str),
    /// `DoublePlantBlock.canSurvive`（下半格走 `VegetationBlock`）：标签判定，
    /// 但放置走 `placeAt` 放上下两格。
    DoublePlant(&'static str),
}

/// 这个状态的存活判定是否已移植。未移植时调用方要把整个 placed feature 标 unsupported，
/// 绝不静默当成 true/false。
///
/// Whether this state's survival check is ported. The caller must mark the whole placed
/// feature unsupported when false; never silently treat it as true or false.
pub fn supports(state: u32) -> bool {
    classify(state).is_some()
}

/// 官方 `BlockState.canSurvive(level, pos)`。只在 [`supports`] 为真时调用。
///
/// Official `BlockState.canSurvive(level, pos)`. Call only when [`supports`] is true.
pub fn can_survive(state: u32, world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let tag = match classify(state) {
        Some(Kind::Default) => return true,
        Some(Kind::Vegetation(tag)) | Some(Kind::DoublePlant(tag)) => tag,
        None => return false,
    };
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    let id = block_id_of_state(below).map_or(-1, |id| id as i32);
    block_tag(tag).is_some_and(|ids| ids.contains(&id))
}

/// 是否走 `DoublePlantBlock.placeAt`（放上下两格）而不是单格 `setBlock`。
///
/// Whether to use `DoublePlantBlock.placeAt` (two blocks) instead of a single `setBlock`.
pub fn is_double_plant(state: u32) -> bool {
    matches!(
        block_of_state(state).map(|block| block.class),
        Some(BlockClass::DoublePlantBlock) | Some(BlockClass::TallFlowerBlock)
    )
}

/// 按官方类名分派；返回 `None` 表示存活判定未移植。
///
/// Dispatch by official block class; `None` means the survival check is unported.
fn classify(state: u32) -> Option<Kind> {
    let class = block_of_state(state)?.class;
    Some(match class {
        // `BlockBehaviour.canSurvive` 默认 true；PumpkinBlock 直接继承 Block。
        // `BlockBehaviour.canSurvive` defaults to true; PumpkinBlock extends Block.
        BlockClass::Block | BlockClass::PumpkinBlock => Kind::Default,
        // 直接继承 VegetationBlock、不覆写 canSurvive 的类，标签判定。
        // Classes extending VegetationBlock without overriding canSurvive: a tag check.
        BlockClass::BushBlock
        | BlockClass::FlowerBlock
        | BlockClass::TallGrassBlock
        | BlockClass::SweetBerryBushBlock
        | BlockClass::EyeblossomBlock
        | BlockClass::FireflyBushBlock
        | BlockClass::SaplingBlock => Kind::Vegetation("minecraft:supports_vegetation"),
        BlockClass::DryVegetationBlock => Kind::Vegetation("minecraft:supports_dry_vegetation"),
        // 配置里的 to_place 都是默认状态（half=lower），canSurvive 走 VegetationBlock。
        // Exported to_place states are all the default (half=lower), so canSurvive uses
        // the VegetationBlock tag check; placement still goes through placeAt.
        BlockClass::DoublePlantBlock | BlockClass::TallFlowerBlock => {
            Kind::DoublePlant("minecraft:supports_vegetation")
        }
        // 蘑菇（亮度）、睡莲（流体）、孢子花（canSupportCenter）、甘蔗、仙人掌、
        // 苔藓地毯等都覆写了 canSurvive/mayPlaceOn，本轮明确不支持。
        // Mushroom (brightness), lily pad (fluid), spore blossom (canSupportCenter),
        // sugar cane, cactus, mossy carpet override canSurvive/mayPlaceOn: unported.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::block_by_name;

    fn state(name: &str) -> u32 {
        block_by_name(name).unwrap().default_state
    }

    #[test]
    fn ported_classes_are_supported() {
        for name in [
            "minecraft:pumpkin",
            "minecraft:melon",
            "minecraft:short_grass",
            "minecraft:dead_bush",
            "minecraft:bush",
            "minecraft:blue_orchid",
            "minecraft:sweet_berry_bush",
            "minecraft:closed_eyeblossom",
            "minecraft:firefly_bush",
            "minecraft:oak_sapling",
            "minecraft:tall_grass",
            "minecraft:sunflower",
            "minecraft:large_fern",
        ] {
            assert!(supports(state(name)), "{name}");
        }
    }

    #[test]
    fn classes_with_special_survival_stay_unported() {
        for name in [
            "minecraft:brown_mushroom",
            "minecraft:red_mushroom",
            "minecraft:lily_pad",
            "minecraft:spore_blossom",
            "minecraft:sugar_cane",
            "minecraft:cactus",
        ] {
            assert!(!supports(state(name)), "{name}");
        }
    }

    #[test]
    fn double_plants_are_flagged_for_place_at() {
        assert!(is_double_plant(state("minecraft:tall_grass")));
        assert!(is_double_plant(state("minecraft:sunflower")));
        assert!(!is_double_plant(state("minecraft:short_grass")));
    }
}
