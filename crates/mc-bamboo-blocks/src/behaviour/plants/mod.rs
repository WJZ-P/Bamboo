//! 植物：`BushBlock` 系列、高植物、作物、茎、蘑菇、珊瑚、荷叶、藤蔓、`GrowingPlant` 系列、竹子、仙人掌、
//! 甘蔗、紫颂、滴叶、垂根、`MultifaceBlock` 系列。
//!
//! 一个文件放一组官方类，文件头写着里面有哪些类。这里只有分派和几个类共用的小函数。
//!
//! 这里不做生长：官方的 `randomTick` 和骨粉都不在范围里。官方取随机值的地方，
//! 比如海带和藤蔓放下时的 `age`、珊瑚死亡的延迟，都取一个固定值，oracle 把这些题排除了。

mod bamboo;
mod cactus;
mod chorus;
mod cocoa;
mod coral;
mod crop;
mod double_plant;
mod dripleaf;
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
    crop::{AttachedStem, Crop},
    double_plant::{DoublePlant, PitcherCrop, SmallDripleaf, TallSeagrass},
    dripleaf::{BigDripleaf, BigDripleafStem},
    growing_plant::{GrowingPlantBody, GrowingPlantHead},
    hanging_roots::HangingRoots,
    multiface::Multiface,
    segmented::{FlowerBed, LeafLitter},
    sugar_cane::SugarCane,
    vegetation::{MangrovePropagule, Mushroom, Seagrass, SweetBerryBush, Vegetation},
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
        | C::BushBlock
        | C::CactusFlowerBlock
        | C::DryVegetationBlock
        | C::EyeblossomBlock
        | C::FireflyBushBlock
        | C::FlowerBlock
        | C::LilyPadBlock
        | C::NetherFungusBlock
        | C::NetherRootsBlock
        | C::NetherSproutsBlock
        | C::NetherWartBlock
        | C::SaplingBlock
        | C::ShortDryGrassBlock
        | C::StemBlock
        | C::TallDryGrassBlock
        | C::TallGrassBlock
        | C::WitherRoseBlock => Some(&Vegetation),
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

/// `tick` 里“放不住就破坏”这一段。
fn break_if_unsupported(state: BlockState, world: &mut World, pos: BlockPos) {
    if !state.can_survive(world, pos) {
        world.destroy_block(pos, UPDATE_LIMIT);
    }
}
