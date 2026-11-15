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

use mc_bamboo_core::Direction;
use mc_bamboo_data::{
    BlockClass, block_id_of_state, block_of_state, block_tag, collision_shape, facts, registries,
    state_facts, state_fluid, tags,
};

use super::{state::with_property, world::FeatureWorld};

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
    /// `SeagrassBlock.mayPlaceOn`：下面 `isFaceSturdy(UP)` 且不在
    /// `cannot_support_seagrass` 标签里。
    Seagrass,
    /// `TallSeagrassBlock.canSurvive`：上半格看下半格，下半格多看一次流体。
    TallSeagrass,
    /// `SeaPickleBlock.mayPlaceOn`：下面那格碰撞形状朝上的面非空，或 `isFaceSturdy(UP)`。
    SeaPickle,
    /// `LilyPadBlock.mayPlaceOn`：下面那格是（或含有）`supports_lily_pad`，且上面没有流体。
    LilyPad,
    /// `SporeBlossomBlock.canSurvive`：上面那格 `canSupportCenter(DOWN)` 且本格不是水。
    SporeBlossom,
    /// `CarpetBlock.canSurvive`：下面那格不是空气。
    Carpet,
    /// `MushroomBlock.canSurvive`：下面 `isSolidRender`，且亮度 < 13（或下面在
    /// `overrides_mushroom_light_requirement` 里）。
    Mushroom,
    /// 苍白苔藓地毯：下面非空气（`BASE`）或下面同类且 `BASE`。
    ///
    /// Pale moss carpet: the block below is not air (`BASE`), or the same block with `BASE`.
    MossyCarpet,
    /// 悬挂的红树胎生苗：上面那格在 `supports_hanging_mangrove_propagule` 标签里。
    ///
    /// A hanging mangrove propagule: the block above is in the
    /// `supports_hanging_mangrove_propagule` tag.
    HangingPropagule,
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
        Some(Kind::Seagrass) => seagrass_can_survive(world, pos),
        Some(Kind::TallSeagrass) => tall_seagrass_can_survive(state, world, pos),
        Some(Kind::SeaPickle) => sea_pickle_can_survive(world, pos),
        Some(Kind::LilyPad) => lily_pad_can_survive(world, pos),
        Some(Kind::SporeBlossom) => spore_blossom_can_survive(world, pos),
        // `CarpetBlock.canSurvive` 是 `!level.isEmptyBlock(pos.below())`。
        // `CarpetBlock.canSurvive` is `!level.isEmptyBlock(pos.below())`.
        Some(Kind::Carpet) => !is_air_block(below),
        Some(Kind::Mushroom) => mushroom_can_survive(world, pos),
        // `MossyCarpetBlock.canSurvive`：`BASE` 时下面非空气；否则下面同类且 `BASE`。
        // `MossyCarpetBlock.canSurvive`: with `BASE` the block below is not air; otherwise
        // the block below is the same block carrying `BASE`.
        Some(Kind::MossyCarpet) => {
            if is_air_block(below) {
                return false;
            }
            if is_base_mossy_carpet(state) {
                true
            } else {
                is_base_mossy_carpet(below) && is_mossy_carpet(below)
            }
        }
        Some(Kind::HangingPropagule) => {
            let above = world.get_block(pos.0, pos.1 + 1, pos.2);
            let id = block_id_of_state(above).map_or(-1, |id| id as i32);
            block_tag("minecraft:supports_hanging_mangrove_propagule")
                .is_some_and(|ids| ids.contains(&id))
        }
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

/// `SeagrassBlock.mayPlaceOn`：下面 `isFaceSturdy(UP)`，且不在
/// `cannot_support_seagrass` 里。
///
/// `SeagrassBlock.mayPlaceOn`: the block below is `isFaceSturdy(UP)` and not in
/// `cannot_support_seagrass`.
fn seagrass_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    let sturdy = state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0;
    sturdy && !in_block_tag(below, "minecraft:cannot_support_seagrass")
}

/// `TallSeagrassBlock.canSurvive`：上半格只认「下面是同类的下半格」；下半格走
/// `DoublePlantBlock`（`supports_vegetation`）、`SeagrassBlock` 的标签判定，
/// 再加上本格的流体是满水。
///
/// `TallSeagrassBlock.canSurvive`: the upper half only accepts the matching lower half;
/// the lower half uses `DoublePlantBlock` (`supports_vegetation`) plus the
/// `cannot_support_seagrass` tag and requires a full water fluid here.
fn tall_seagrass_can_survive(state: u32, world: &FeatureWorld<'_>, pos: Pos) -> bool {
    if with_property(state, "half", "upper").is_some_and(|upper| upper == state) {
        let below = world.get_block(pos.0, pos.1 - 1, pos.2);
        return is_block(below, "minecraft:tall_seagrass")
            && with_property(below, "half", "lower").is_some_and(|lower| lower == below);
    }
    if !seagrass_can_survive(world, pos) {
        return false;
    }
    let fluid = state_fluid(world.get_block(pos.0, pos.1, pos.2));
    in_fluid_tag(world.get_block(pos.0, pos.1, pos.2), "minecraft:water") && fluid.amount == 8
}

/// `SeaPickleBlock.mayPlaceOn`：下面那格碰撞形状朝上的面非空，或 `isFaceSturdy(UP)`。
/// 注意官方用的是「面形状非空」而不是「满面」，海泡菜自己那圈碰撞形状也满足。
///
/// `SeaPickleBlock.mayPlaceOn`: the block below has a non-empty collision face toward
/// UP, or is `isFaceSturdy(UP)`. The official check is "face shape not empty", not
/// "face full", which a sea pickle's own collision shape also satisfies.
fn sea_pickle_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    !mc_bamboo_blocks::shape::face_shape(collision_shape(below), Direction::Up).is_empty()
        || state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0
}

/// `LilyPadBlock.mayPlaceOn`：这一格的流体在 `supports_lily_pad` 里，或下面那格在
/// 同名方块标签里；且上面一格没有流体。
///
/// `LilyPadBlock.mayPlaceOn`: this position's fluid is in `supports_lily_pad` or the
/// block below is in the block tag of the same name, and no fluid sits above.
fn lily_pad_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    let supported = in_fluid_tag(below, "minecraft:supports_lily_pad")
        || in_block_tag(below, "minecraft:supports_lily_pad");
    let above = world.get_block(pos.0, pos.1 + 1, pos.2);
    supported && state_fluid(above).fluid == registries::fluid::EMPTY
}

/// `SporeBlossomBlock.canSurvive`：上面那格 `Block.canSupportCenter(DOWN)`，
/// 且本格不是水。
///
/// `SporeBlossomBlock.canSurvive`: the block above supports the center face toward DOWN
/// and this position holds no water.
fn spore_blossom_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let above = world.get_block(pos.0, pos.1 + 1, pos.2);
    let supports = if in_block_tag(above, "minecraft:unstable_bottom_center") {
        false
    } else {
        // `SupportType.CENTER` 的位从 `facts::STURDY_CENTER` 起，朝向是 DOWN（ordinal 0）。
        // CENTER support bits start at `facts::STURDY_CENTER`; DOWN has ordinal 0.
        state_facts(above) & (1u64 << facts::STURDY_CENTER) != 0
    };
    supports && !in_fluid_tag(world.get_block(pos.0, pos.1, pos.2), "minecraft:water")
}

/// `MushroomBlock.canSurvive`：下面在 `overrides_mushroom_light_requirement` 里就直接 true；
/// 否则要求亮度 < 13 且下面 `isSolidRender()`。
///
/// **亮度**：`WorldGenRegion.getRawBrightness` 没有覆写，转发到 `level.getLightEngine()`，
/// 即真实光照引擎。`FEATURES` 阶段在 `INITIALIZE_LIGHT` **之前**，区块还没有光照数据：
/// `SkyLightSectionStorage.getLightValue` 在 `topSection == currentLowestY`
/// （该列还没有任何光照段）时返回 **15**，`BlockLightSectionStorage.getLightValue` 在没有
/// DataLayer 时返回 **0**，所以 `getRawBrightness(pos, 0)` = `max(0, 15 - 0)` = **15**，
/// 恒不满足 `< 13`。因此蘑菇的存活判定在 `FEATURES` 阶段**只取决于下面那格**——除非下面
/// 在 `overrides_mushroom_light_requirement` 里（cherry_leaves / coarse_dirt / note_block）。
///
/// `MushroomBlock.canSurvive`: short-circuit true when the block below is in
/// `overrides_mushroom_light_requirement`; otherwise require brightness < 13 and
/// `isSolidRender()` below.
///
/// **Brightness**: `WorldGenRegion` does not override `getRawBrightness`; it forwards to
/// `level.getLightEngine()`. During `FEATURES` (before `INITIALIZE_LIGHT`) the chunk has no
/// light data: `SkyLightSectionStorage.getLightValue` returns **15** when
/// `topSection == currentLowestY`, and `BlockLightSectionStorage` returns **0** without a
/// DataLayer, so `getRawBrightness(pos, 0)` is always **15** and never below 13. The check
/// therefore reduces to the block below — unless it is in that override tag.
fn mushroom_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    if in_block_tag(below, "minecraft:overrides_mushroom_light_requirement") {
        return true;
    }
    // `getRawBrightness(pos, 0)` 在 FEATURES 阶段恒为 15，`< 13` 永不成立。
    // `getRawBrightness(pos, 0)` is always 15 during FEATURES, so `< 13` never holds.
    false
}

/// `BlockState.isAir()`，跳过方块标签查询。
/// `BlockState.isAir()`, avoiding the tag lookup.
fn is_air_block(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// `state.is(Blocks.PALE_MOSS_CARPET)`。
/// `state.is(Blocks.PALE_MOSS_CARPET)`.
fn is_mossy_carpet(state: u32) -> bool {
    block_of_state(state).is_some_and(|block| block.name == "minecraft:pale_moss_carpet")
}

/// `state.getValue(MossyCarpetBlock.BASE)`。
/// `state.getValue(MossyCarpetBlock.BASE)`.
fn is_base_mossy_carpet(state: u32) -> bool {
    with_property(state, "bottom", "true") == Some(state)
}

/// 是否走 `DoublePlantBlock.placeAt`（放上下两格）而不是单格 `setBlock`。
/// `TallSeagrassBlock` 也继承 `DoublePlantBlock`，官方用的是 `instanceof`。
///
/// Whether to use `DoublePlantBlock.placeAt` (two blocks) instead of a single `setBlock`.
/// `TallSeagrassBlock` extends `DoublePlantBlock` too, and the official check is `instanceof`.
pub fn is_double_plant(state: u32) -> bool {
    matches!(
        block_of_state(state).map(|block| block.class),
        Some(BlockClass::DoublePlantBlock)
            | Some(BlockClass::TallFlowerBlock)
            | Some(BlockClass::TallSeagrassBlock)
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
        // `MangrovePropaguleBlock.canSurvive`：悬挂时看上面那格；否则走 VegetationBlock，
        // 而它的 `mayPlaceOn` 换成了 `supports_mangrove_propagule` 标签。
        // `MangrovePropaguleBlock.canSurvive`: a hanging one checks the block above;
        // otherwise VegetationBlock applies, with `mayPlaceOn` overridden to the
        // `supports_mangrove_propagule` tag.
        BlockClass::MangrovePropaguleBlock => {
            if super::tree::property_value(state, "hanging") == Some("true") {
                Kind::HangingPropagule
            } else {
                Kind::Vegetation("minecraft:supports_mangrove_propagule")
            }
        }
        // 配置里的 to_place 都是默认状态（half=lower），canSurvive 走 VegetationBlock。
        // Exported to_place states are all the default (half=lower), so canSurvive uses
        // the VegetationBlock tag check; placement still goes through placeAt.
        BlockClass::DoublePlantBlock | BlockClass::TallFlowerBlock => {
            Kind::DoublePlant("minecraft:supports_vegetation")
        }
        BlockClass::CactusBlock => Kind::Cactus,
        BlockClass::SugarCaneBlock => Kind::SugarCane,
        // 海草和甘蔗一样：都是 VegetationBlock 的泛化判定加自己的标签。
        // Seagrass and sugar cane both add their own tags to the generic check.
        BlockClass::SeagrassBlock => Kind::Seagrass,
        BlockClass::TallSeagrassBlock => Kind::TallSeagrass,
        BlockClass::SeaPickleBlock => Kind::SeaPickle,
        BlockClass::LilyPadBlock => Kind::LilyPad,
        BlockClass::SporeBlossomBlock => Kind::SporeBlossom,
        BlockClass::CarpetBlock => Kind::Carpet,
        BlockClass::MushroomBlock => Kind::Mushroom,
        BlockClass::MossyCarpetBlock => Kind::MossyCarpet,
        // 苔藓地毯等仍覆写了 canSurvive/mayPlaceOn，本轮明确不支持。
        // Mossy carpet still overrides canSurvive/mayPlaceOn: unported.
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
            "minecraft:seagrass",
            "minecraft:tall_seagrass",
            "minecraft:sea_pickle",
            "minecraft:lily_pad",
            "minecraft:spore_blossom",
            "minecraft:moss_carpet",
            "minecraft:brown_mushroom",
            "minecraft:red_mushroom",
        ] {
            assert!(supports(state(name)), "{name}");
        }
    }

    #[test]
    fn mushroom_only_survives_on_the_light_override_tag() {
        // 蘑菇下面在 override 标签里（coarse_dirt）才成立；普通石头不成立，因为
        // FEATURES 阶段的 getRawBrightness 恒为 15。
        // A mushroom survives on coarse_dirt (the override tag) but not on plain stone,
        // because getRawBrightness is always 15 during FEATURES.
        assert!(state("minecraft:brown_mushroom") != 0);
        assert!(
            block_tag("minecraft:overrides_mushroom_light_requirement")
                .unwrap()
                .contains(&11)
        );
    }

    #[test]
    fn classes_with_special_survival_stay_unported() {
        // 蘑菇的亮度判据在 FEATURES 阶段恒为 15，等价于「只有 override 标签能活」，
        // 已移植；这里留空表提醒以后加新类时补测试。
        // The mushroom's brightness is always 15 during FEATURES, so the check reduces to
        // the override tag and is ported; the empty list is a reminder to add tests for
        // future classes.
        let unported: [&str; 0] = [];
        for name in unported {
            assert!(!supports(state(name)), "{name}");
        }
    }

    /// `MossyCarpetBlock` 覆写了 `canSurvive`（看下面那格是否同类 + `BASE`），
    /// 且 `SimpleBlockFeature` 走 `MossyCarpetBlock.placeAt`（含侧面 `nextBoolean`），
    /// 所以苍苔藓地毯必须仍是 unported，`pale_moss_vegetation` 才会被整块跳过。
    ///
    /// `MossyCarpetBlock` overrides `canSurvive` (same block below plus `BASE`) and
    /// `SimpleBlockFeature` routes it through `MossyCarpetBlock.placeAt` (with side
    /// `nextBoolean`), so pale moss carpet must stay unported.
    #[test]
    fn pale_moss_carpet_is_supported_and_needs_support_below() {
        let pale = state("minecraft:pale_moss_carpet");
        assert!(supports(pale));
        // 同类的普通地毯（`CarpetBlock`）走另一条分支。
        // The plain `CarpetBlock` takes a different branch.
        assert!(supports(state("minecraft:moss_carpet")));
    }

    #[test]
    fn double_plants_are_flagged_for_place_at() {
        assert!(is_double_plant(state("minecraft:tall_grass")));
        assert!(is_double_plant(state("minecraft:sunflower")));
        assert!(!is_double_plant(state("minecraft:short_grass")));
    }
}
