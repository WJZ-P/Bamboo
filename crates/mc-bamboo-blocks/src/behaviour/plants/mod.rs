//! 植物：`BushBlock` 系列、高植物、作物、茎、蘑菇、珊瑚、荷叶、藤蔓、`GrowingPlant` 系列、竹子、仙人掌、
//! 甘蔗、紫颂、滴叶、垂根、`MultifaceBlock` 系列。
//!
//! 一个文件放一组官方类，文件头写着里面有哪些类。这里只有分派和几个类共用的小函数。
//!
//! 生长都在随机刻和骨粉里，随机数和官方逐位相同。要世界生成的地物才能长的不做：树苗长成树、
//! 巨型蘑菇和菌类、骨粉催熟草方块和杜鹃，见 [`vegetation`] 和 oracle 的排除表。

mod bamboo;
mod cactus;
mod chorus;
mod cocoa;
mod coral;
mod crop;
mod double_plant;
mod dripleaf;
mod grass;
mod growing_plant;
mod hanging_roots;
mod multiface;
mod segmented;
mod sugar_cane;
mod vegetation;
mod vine;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::BlockClass;

use self::{
    bamboo::{BambooSapling, BambooStalk},
    cactus::Cactus,
    chorus::{ChorusFlower, ChorusPlant},
    cocoa::Cocoa,
    coral::{BaseCoralPlant, BaseCoralWallFan, Coral, CoralPlant, CoralWallFan},
    crop::{AttachedStem, Crop, Stem},
    double_plant::{DoublePlant, PitcherCrop, SmallDripleaf, TallSeagrass},
    dripleaf::{BigDripleaf, BigDripleafStem},
    grass::Grass,
    growing_plant::{GrowingPlantBody, GrowingPlantHead},
    hanging_roots::HangingRoots,
    multiface::Multiface,
    segmented::{FlowerBed, LeafLitter},
    sugar_cane::SugarCane,
    vegetation::{
        MangrovePropagule, Mushroom, NetherFungus, NetherWart, Sapling, Seagrass, SweetBerryBush,
        Vegetation,
    },
    vine::Vine,
};
use super::Behaviour;
use crate::{
    state::BlockState,
    world::{UPDATE_LIMIT, World},
};

/// 这个家族里类到行为的分派。新版本多出来、还没移植的类是 `None`。
pub(super) fn of(class: BlockClass) -> Option<&'static dyn Behaviour> {
    use BlockClass as C;

    match class {
        C::AttachedStemBlock => Some(&AttachedStem),
        C::AzaleaBlock
        | C::CactusFlowerBlock
        | C::DryVegetationBlock
        | C::EyeblossomBlock
        | C::FlowerBlock
        | C::LilyPadBlock
        | C::NetherRootsBlock
        | C::NetherSproutsBlock
        | C::WitherRoseBlock => Some(&Vegetation),
        C::BushBlock
        | C::FireflyBushBlock
        | C::ShortDryGrassBlock
        | C::TallDryGrassBlock
        | C::TallGrassBlock => Some(&Grass),
        C::NetherFungusBlock => Some(&NetherFungus),
        C::NetherWartBlock => Some(&NetherWart),
        C::SaplingBlock => Some(&Sapling),
        C::StemBlock => Some(&Stem),
        C::BambooSaplingBlock => Some(&BambooSapling),
        C::BambooStalkBlock => Some(&BambooStalk),
        C::BaseCoralFanBlock | C::BaseCoralPlantBlock => Some(&BaseCoralPlant),
        C::BaseCoralWallFanBlock => Some(&BaseCoralWallFan),
        C::BeetrootBlock
        | C::CarrotBlock
        | C::CropBlock
        | C::PotatoBlock
        | C::TorchflowerCropBlock => Some(&Crop),
        C::BigDripleafBlock => Some(&BigDripleaf),
        C::BigDripleafStemBlock => Some(&BigDripleafStem),
        C::CactusBlock => Some(&Cactus),
        C::CaveVinesBlock | C::KelpBlock | C::TwistingVinesBlock | C::WeepingVinesBlock => {
            Some(&GrowingPlantHead)
        }
        C::CaveVinesPlantBlock
        | C::KelpPlantBlock
        | C::TwistingVinesPlantBlock
        | C::WeepingVinesPlantBlock => Some(&GrowingPlantBody),
        C::ChorusFlowerBlock => Some(&ChorusFlower),
        C::ChorusPlantBlock => Some(&ChorusPlant),
        C::CocoaBlock => Some(&Cocoa),
        C::CoralBlock => Some(&Coral),
        C::CoralFanBlock | C::CoralPlantBlock => Some(&CoralPlant),
        C::CoralWallFanBlock => Some(&CoralWallFan),
        C::DoublePlantBlock | C::TallFlowerBlock => Some(&DoublePlant),
        C::FlowerBedBlock => Some(&FlowerBed),
        C::GlowLichenBlock | C::MultifaceBlock | C::SculkVeinBlock => Some(&Multiface),
        C::HangingRootsBlock => Some(&HangingRoots),
        C::LeafLitterBlock => Some(&LeafLitter),
        C::MangrovePropaguleBlock => Some(&MangrovePropagule),
        C::MushroomBlock => Some(&Mushroom),
        C::PitcherCropBlock => Some(&PitcherCrop),
        C::SeagrassBlock => Some(&Seagrass),
        C::SmallDripleafBlock => Some(&SmallDripleaf),
        C::SugarCaneBlock => Some(&SugarCane),
        C::SweetBerryBushBlock => Some(&SweetBerryBush),
        C::TallSeagrassBlock => Some(&TallSeagrass),
        C::VineBlock => Some(&Vine),
        _ => None,
    }
}

/// `updateShape` 里“放不住就排一个刻”这一段，到时候由 [`break_if_unsupported`] 判定。
fn schedule_break_if_unsupported(state: BlockState, world: &mut World, pos: BlockPos) {
    if !state.can_survive(world, pos) {
        world.schedule_tick(pos, state.block(), 1);
    }
}

/// 蘑菇和藤蔓蔓延前数的范围：水平 4 格，上下 1 格。
const SPREAD_SEARCH_RADIUS: i32 = 4;
/// 范围里同种方块（算上自己）多于这个数就不蔓延，`atMostMatched(4)`。
const SPREAD_MAX_NEIGHBOURS: i32 = 4;

/// `MushroomBlock.canSpread` 和 `VineBlock.canSpread`：周围 9×3×9 的范围里同种方块不多。
fn can_spread(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let mut found = 0;
    for x in -SPREAD_SEARCH_RADIUS..=SPREAD_SEARCH_RADIUS {
        for y in -1..=1 {
            for z in -SPREAD_SEARCH_RADIUS..=SPREAD_SEARCH_RADIUS {
                if world.block_state(pos.offset(x, y, z)).is_same_block(state) {
                    found += 1;
                    if found > SPREAD_MAX_NEIGHBOURS {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `tick` 里“放不住就破坏”这一段。
fn break_if_unsupported(state: BlockState, world: &mut World, pos: BlockPos) {
    if !state.can_survive(world, pos) {
        world.destroy_block(pos, true, UPDATE_LIMIT);
    }
}
