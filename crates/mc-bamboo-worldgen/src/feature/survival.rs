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
    BlockClass, block_id_of_state, block_of_state, block_tag, collision_shape, facts, registries,
    state_facts, state_fluid,
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
    /// `LilyPadBlock.mayPlaceOn`（`VegetationBlock.canSurvive` 转发到它）：下面那格在
    /// `#minecraft:supports_lily_pad` 的方块标签或流体标签里，且本格没有流体。
    LilyPad,
    /// `SeaPickleBlock.canSurvive`：下面那格的碰撞形状 `UP` 面非空或
    /// `isFaceSturdy(FULL, UP)`。
    SeaPickle,
    /// `SporeBlossomBlock.canSurvive`：上面那格 `canSupportCenter(DOWN)`，且这一格不是水。
    SporeBlossom,
    /// `SeagrassBlock.mayPlaceOn`：下面那格 `isFaceSturdy(FULL, UP)` 且不在
    /// `#minecraft:cannot_support_seagrass` 里。
    Seagrass,
    /// `TallSeagrassBlock.canSurvive`（下半格）：`VegetationBlock` 标签判定 + 本格是满水。
    TallSeagrass,
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
        // `LilyPadBlock.mayPlaceOn`：`(below 的流体在 supports_lily_pad 流体标签里 ||
        // below 的方块在 supports_lily_pad 方块标签里) && 本格的流体是 EMPTY`。官方把
        // `mayPlaceOn` 的 `pos` 设成 below，所以 `pos.above()` 就是睡莲自己的位置。
        // `LilyPadBlock.mayPlaceOn`: the fluid at `below` is in the fluid tag, or the block
        // is in the block tag `supports_lily_pad`, and the fluid at the lily pad is EMPTY.
        // The official `pos` argument is `below`, so `pos.above()` is the lily pad itself.
        Some(Kind::LilyPad) => {
            let supported = block_tag("minecraft:supports_lily_pad")
                .is_some_and(|ids| ids.contains(&block_id_of(below)))
                || fluid_tag("minecraft:supports_lily_pad")
                    .is_some_and(|ids| ids.contains(&state_fluid(below).fluid));
            // `fluidAbove` 是 `level.getFluidState(pos)`，即睡莲自己那一格的世界流体。
            // `fluidAbove` is `level.getFluidState(pos)`, the world fluid at the lily pad.
            supported
                && state_fluid(world.get_block(pos.0, pos.1, pos.2)).fluid
                    == registries::fluid::EMPTY
        }
        // `SeaPickleBlock.canSurvive`：`below.getCollisionShape(level, below).getFaceShape(UP)`
        // 非空，或 `below.isFaceSturdy(level, below, UP)`。`getFaceShape` 在形状顶面到达
        // y=1 时非空，所以这里查碰撞形状里有没有顶面贴到 1 的盒子。
        // `SeaPickleBlock.canSurvive`: the UP face shape of `below`'s collision shape is not
        // empty, or `below.isFaceSturdy(UP)`. The face shape is non-empty when the shape
        // reaches y=1, so check for a box whose top sits at 1.
        Some(Kind::SeaPickle) => sea_pickle_placeable(below),
        // `SporeBlossomBlock.canSurvive`：`Block.canSupportCenter(level, pos.above(), DOWN)`
        // 且 `!level.isWaterAt(pos)`。`canSupportCenter` 走 `SupportType.CENTER`，
        // `UNSTABLE_BOTTOM_CENTER` 标签里的方块不算。
        // `SporeBlossomBlock.canSurvive`: `canSupportCenter(pos.above(), DOWN)` and the
        // position is not water. `canSupportCenter` uses `SupportType.CENTER` and rejects
        // blocks in the `unstable_bottom_center` tag.
        Some(Kind::SporeBlossom) => {
            let above = world.get_block(pos.0, pos.1 + 1, pos.2);
            let unstable = block_tag("minecraft:unstable_bottom_center")
                .is_some_and(|ids| ids.contains(&block_id_of(above)));
            state_facts(above) & (1u64 << facts::STURDY_CENTER) != 0
                && !unstable
                && !is_water(state)
        }
        // `SeagrassBlock.mayPlaceOn`：`below.isFaceSturdy(UP) && !below.is(cannot_support_seagrass)`。
        // `SeagrassBlock.mayPlaceOn`: `isFaceSturdy(UP)` and not in
        // `#minecraft:cannot_support_seagrass`.
        Some(Kind::Seagrass) => {
            let cannot_support = block_tag("minecraft:cannot_support_seagrass")
                .is_some_and(|ids| ids.contains(&block_id_of(below)));
            state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0 && !cannot_support
        }
        // `TallSeagrassBlock.canSurvive`（half=lower）：`DoublePlantBlock.canSurvive` →
        // `VegetationBlock.canSurvive` → 覆写后的 `mayPlaceOn`（`isFaceSturdy(UP)` 且不在
        // `cannot_support_seagrass` 里），再加上本格流体是满水的 `FluidTags.WATER`。
        // `TallSeagrassBlock.canSurvive` (half=lower): the VegetationBlock check via the
        // overridden `mayPlaceOn` (`isFaceSturdy(UP)`, not in `cannot_support_seagrass`) plus
        // a full `FluidTags.WATER` fluid at this position.
        Some(Kind::TallSeagrass) => {
            let cannot_support = block_tag("minecraft:cannot_support_seagrass")
                .is_some_and(|ids| ids.contains(&block_id_of(below)));
            let supported =
                state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0 && !cannot_support;
            let fluid = state_fluid(state);
            supported
                && fluid_tag("minecraft:water").is_some_and(|ids| ids.contains(&fluid.fluid))
                && fluid.amount == 8
        }
        None => false,
    }
}

/// 是否走 `DoublePlantBlock.placeAt`（放上下两格）而不是单格 `setBlock`。官方判定是
/// `stateToPlace.getBlock() instanceof DoublePlantBlock`，所以按祖先表查，
/// `TallSeagrassBlock` 这种子类也算。
///
/// Whether to use `DoublePlantBlock.placeAt` (two blocks) instead of a single `setBlock`.
/// The official test is `instanceof DoublePlantBlock`, so check the ancestor table; subclasses
/// such as `TallSeagrassBlock` count too.
pub fn is_double_plant(state: u32) -> bool {
    block_of_state(state).is_some_and(|block| {
        block
            .class
            .ancestors()
            .contains(&mc_bamboo_data::BlockAncestor::DoublePlantBlock)
    })
}

/// 按官方类名分派；返回 `None` 表示存活判定未移植。
///
/// Dispatch by official block class; `None` means the survival check is unported.
fn classify(state: u32) -> Option<Kind> {
    let class = block_of_state(state)?.class;
    Some(match class {
        // `BlockBehaviour.canSurvive` 默认 true；PumpkinBlock 直接继承 Block，
        // SnowyBlock（podzol）也不覆写。
        // `BlockBehaviour.canSurvive` defaults to true; PumpkinBlock extends Block and
        // SnowyBlock (podzol) does not override it either.
        BlockClass::Block | BlockClass::PumpkinBlock | BlockClass::SnowyBlock => Kind::Default,
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
        // `TallSeagrassBlock.canSurvive`（下半格）：`VegetationBlock.canSurvive` 之外还要
        // 本格流体是满水的 `FluidTags.WATER`；导出配置里 to_place 是 half=lower。
        // `TallSeagrassBlock.canSurvive` (lower half): the VegetationBlock check plus a full
        // water fluid at this position. Exported to_place states are all half=lower.
        BlockClass::TallSeagrassBlock => Kind::TallSeagrass,
        // `SeagrassBlock` 不覆写 canSurvive，走 `VegetationBlock` 的标签判定
        // （`mayPlaceOn` 覆写成 isFaceSturdy(UP) && !CANNOT_SUPPORT_SEAGRASS）。
        // `SeagrassBlock` overrides `mayPlaceOn` to isFaceSturdy(UP) without the
        // `cannot_support_seagrass` tag.
        BlockClass::SeagrassBlock => Kind::Seagrass,
        // 睡莲要「下面有 supports_lily_pad 的流体/方块，且自己那格没有流体」。
        // Lily pads need `supports_lily_pad` below and an empty fluid at their own position.
        BlockClass::LilyPadBlock => Kind::LilyPad,
        // 海泡菜的存活判定是「下面的碰撞形状有 UP 面或 isFaceSturdy(UP)」。
        // Sea pickles survive on a block whose collision shape has a UP face.
        BlockClass::SeaPickleBlock => Kind::SeaPickle,
        // 孢子花要上面那格 canSupportCenter(DOWN) 且自己不在水里。
        // Spore blossoms need `canSupportCenter(DOWN)` above and no water at their position.
        BlockClass::SporeBlossomBlock => Kind::SporeBlossom,
        // 蘑菇（亮度）、甘蔗、仙人掌、苔藓地毯等都覆写了 canSurvive/mayPlaceOn，
        // 本轮明确不支持。
        // Mushroom (brightness), sugar cane, cactus and mossy carpet override
        // canSurvive/mayPlaceOn: unported.
        _ => return None,
    })
}

/// `LevelReader.isWaterAt(pos)`：这一格流体在 `FluidTags.WATER` 里。
///
/// `LevelReader.isWaterAt(pos)`: the fluid here is in `FluidTags.WATER`.
fn is_water(state: u32) -> bool {
    fluid_tag("minecraft:water").is_some_and(|ids| ids.contains(&state_fluid(state).fluid))
}

/// 下面那格的方块 ID；范围外返回 -1（不在任何标签里）。
///
/// The block ID of a state; -1 out of range (in no tag).
fn block_id_of(state: u32) -> i32 {
    block_id_of_state(state).map_or(-1, |id| id as i32)
}

/// `HolderSet<Fluid>` 标签名对应的流体注册表 ID 列表。
///
/// The fluid registry IDs of a fluid tag.
fn fluid_tag(name: &str) -> Option<&'static [i32]> {
    let group = mc_bamboo_data::tags::TAG_GROUPS.get(mc_bamboo_data::tags::fluid_tags::GROUP)?;
    let tag = group.tags.iter().find(|tag| tag.name == name)?;
    Some(tag.ids)
}

/// `SeaPickleBlock.mayPlaceOn`：`!collisionShape.getFaceShape(UP).isEmpty() ||
/// isFaceSturdy(UP)`。碰撞形状由生成器在 `EmptyBlockGetter` 上算好，UP 面非空等价于
/// 某个盒子顶面到达 y=1。
///
/// `SeaPickleBlock.mayPlaceOn`: the UP face shape of the collision shape is not empty, or
/// `isFaceSturdy(UP)`. The UP face is non-empty when some box reaches y=1.
fn sea_pickle_placeable(below: u32) -> bool {
    let face_reaches_top = collision_shape(below)
        .iter()
        .any(|box_| box_.max_y >= 1.0 - 1.0e-7);
    face_reaches_top || state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0
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
        ] {
            assert!(supports(state(name)), "{name}");
        }
    }

    #[test]
    fn classes_with_special_survival_stay_unported() {
        for name in [
            "minecraft:brown_mushroom",
            "minecraft:red_mushroom",
            "minecraft:sugar_cane",
            "minecraft:cactus",
        ] {
            assert!(!supports(state(name)), "{name}");
        }
    }

    #[test]
    fn ports_seagrass_lily_pad_pickle_and_spore_blossom() {
        for name in [
            "minecraft:seagrass",
            "minecraft:tall_seagrass",
            "minecraft:lily_pad",
            "minecraft:sea_pickle",
            "minecraft:spore_blossom",
        ] {
            assert!(supports(state(name)), "{name}");
        }
    }

    #[test]
    fn double_plants_are_flagged_for_place_at() {
        assert!(is_double_plant(state("minecraft:tall_grass")));
        assert!(is_double_plant(state("minecraft:sunflower")));
        assert!(!is_double_plant(state("minecraft:short_grass")));
    }
}
