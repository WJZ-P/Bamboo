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

use mc_bamboo_data::{
    BlockClass, block_id_of_state, block_of_state, block_tag, facts, state_facts, state_fluid, tags,
};

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
    /// `LeafLitterBlock.canSurvive`：下面那格 `isFaceSturdy(FULL, UP)`。
    SturdyFaceUp,
    /// `CactusBlock.canSurvive`：四周、下面、上面三处判定。
    Cactus,
    /// `SugarCaneBlock.canSurvive`：下面和相邻流体判定。
    SugarCane,
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
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    match classify(state) {
        Some(Kind::Default) => true,
        Some(Kind::Vegetation(tag)) | Some(Kind::DoublePlant(tag)) => {
            let id = block_id_of_state(below).map_or(-1, |id| id as i32);
            block_tag(tag).is_some_and(|ids| ids.contains(&id))
        }
        // `LeafLitterBlock.canSurvive`：`below.isFaceSturdy(level, below, Direction.UP)`，
        // 即 `isFaceSturdy(FULL)`；Direction.UP 的 ordinal 是 1。
        // `LeafLitterBlock.canSurvive`: `isFaceSturdy(FULL, UP)`; UP's ordinal is 1.
        Some(Kind::SturdyFaceUp) => state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0,
        Some(Kind::Cactus) => cactus_can_survive(world, pos),
        Some(Kind::SugarCane) => sugar_cane_can_survive(world, pos),
        None => false,
    }
}

/// `Direction.Plane.HORIZONTAL` 的四个水平偏移；这些判定只读方块，顺序不影响结果。
///
/// The four `Direction.Plane.HORIZONTAL` offsets; these checks only read blocks,
/// so order does not change the result.
const HORIZONTAL: [(i32, i32, i32); 4] = [(0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

fn block_id(state: u32) -> i32 {
    block_id_of_state(state).map_or(-1, |id| id as i32)
}

fn is_block(state: u32, name: &str) -> bool {
    block_of_state(state).is_some_and(|block| block.name == name)
}

fn in_block_tag(state: u32, tag: &str) -> bool {
    block_tag(tag).is_some_and(|ids| ids.contains(&block_id(state)))
}

/// `FluidState.is(tag)`：状态里的流体 ID 是否在该流体标签里。
///
/// `FluidState.is(tag)`: whether the state's fluid ID belongs to the fluid tag.
fn in_fluid_tag(state: u32, tag: &str) -> bool {
    let fluid = state_fluid(state).fluid;
    let group = &tags::TAG_GROUPS[tags::fluid_tags::GROUP];
    group
        .tags
        .iter()
        .find(|entry| entry.name == tag)
        .is_some_and(|entry| entry.ids.contains(&fluid))
}

/// `CactusBlock.canSurvive`：水平相邻不能有实心方块或岩浆；下面得是仙人掌本身或
/// `supports_cactus` 标签；上面那一格不能是 `liquid()`。
///
/// `CactusBlock.canSurvive`: no solid/lava horizontal neighbour; below must be a
/// cactus or the `supports_cactus` tag; the block above must not be `liquid()`.
fn cactus_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    for (dx, _, dz) in HORIZONTAL {
        let neighbor = world.get_block(pos.0 + dx, pos.1, pos.2 + dz);
        if state_facts(neighbor) & facts::SOLID != 0 || in_fluid_tag(neighbor, "minecraft:lava") {
            return false;
        }
    }
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    let supported =
        is_block(below, "minecraft:cactus") || in_block_tag(below, "minecraft:supports_cactus");
    let above = world.get_block(pos.0, pos.1 + 1, pos.2);
    supported && state_facts(above) & facts::LIQUID == 0
}

/// `SugarCaneBlock.canSurvive`：下面是甘蔗则恒 true；否则下面得在
/// `supports_sugar_cane` 标签里，且水平相邻的某格流体或方块在
/// `supports_sugar_cane_adjacently` 标签里。
///
/// `SugarCaneBlock.canSurvive`: sugar cane below is always true; otherwise the
/// block below must be in `supports_sugar_cane` and a horizontal neighbour must
/// be in `supports_sugar_cane_adjacently` (fluid or block tag).
fn sugar_cane_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    if is_block(below, "minecraft:sugar_cane") {
        return true;
    }
    if !in_block_tag(below, "minecraft:supports_sugar_cane") {
        return false;
    }
    for (dx, _, dz) in HORIZONTAL {
        let side = world.get_block(pos.0 + dx, pos.1 - 1, pos.2 + dz);
        if in_fluid_tag(side, "minecraft:supports_sugar_cane_adjacently")
            || in_block_tag(side, "minecraft:supports_sugar_cane_adjacently")
        {
            return true;
        }
    }
    false
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
        | BlockClass::SaplingBlock
        | BlockClass::FlowerBedBlock => Kind::Vegetation("minecraft:supports_vegetation"),
        BlockClass::DryVegetationBlock
        | BlockClass::ShortDryGrassBlock
        | BlockClass::TallDryGrassBlock => Kind::Vegetation("minecraft:supports_dry_vegetation"),
        // LeafLitterBlock 覆写 canSurvive 为 isFaceSturdy(UP)。
        // LeafLitterBlock overrides canSurvive with isFaceSturdy(UP).
        BlockClass::LeafLitterBlock => Kind::SturdyFaceUp,
        // 配置里的 to_place 都是默认状态（half=lower），canSurvive 走 VegetationBlock。
        // Exported to_place states are all the default (half=lower), so canSurvive uses
        // the VegetationBlock tag check; placement still goes through placeAt.
        BlockClass::DoublePlantBlock | BlockClass::TallFlowerBlock => {
            Kind::DoublePlant("minecraft:supports_vegetation")
        }
        BlockClass::CactusBlock => Kind::Cactus,
        BlockClass::SugarCaneBlock => Kind::SugarCane,
        // 蘑菇（亮度）、睡莲（流体）、孢子花（canSupportCenter）、
        // 苔藓地毯等仍覆写了 canSurvive/mayPlaceOn，本轮明确不支持。
        // Mushroom (brightness), lily pad (fluid), spore blossom (canSupportCenter),
        // mossy carpet still override canSurvive/mayPlaceOn: unported.
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
            "minecraft:wildflowers",
            "minecraft:short_dry_grass",
            "minecraft:tall_dry_grass",
            "minecraft:leaf_litter",
            "minecraft:sugar_cane",
            "minecraft:cactus",
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
