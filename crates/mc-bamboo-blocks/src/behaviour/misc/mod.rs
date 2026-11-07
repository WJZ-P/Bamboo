//! 其余方块：雪层、蜡烛、蜡烛蛋糕、蛋糕、海泡菜、海龟蛋、脚手架、滴水石锥、紫晶簇、混凝土粉末、
//! 下落方块、炼药锅、堆肥桶、花盆、火、液体、耕地、土径、树叶、海绵、TNT、重生锚，以及不属于
//! 其他家族的类。
//!
//! 一个文件放一组官方类，文件头写着里面有哪些类。这里只有分派和几个类共用的小函数。

mod amethyst;
mod cake;
mod candle;
mod cauldron;
mod composter;
mod copper;
mod crafter;
mod creaking_heart;
mod decorated_pot;
mod facing;
mod falling;
pub(crate) mod fire;
mod flower_pot;
mod frogspawn;
mod frosted_ice;
mod hanging;
mod jukebox;
mod leaves;
mod liquid;
mod menu;
mod portal;
mod potent_sulfur;
mod pumpkin;
mod redstone_ore;
mod respawn_anchor;
mod scaffolding;
mod sculk;
mod sea_pickle;
mod snow;
mod soil;
mod speleothem;
mod sponge;
mod test_block;
mod tnt;
mod turtle_egg;
mod waterlogged;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::BlockClass;

use self::{
    amethyst::AmethystCluster,
    cake::{Cake, CandleCake},
    candle::Candle,
    cauldron::Cauldron,
    composter::Composter,
    copper::{CopperBulb, CopperGolemStatue, WeatheringCopperGolemStatue},
    crafter::Crafter,
    creaking_heart::CreakingHeart,
    decorated_pot::DecoratedPot,
    facing::{EndPortalFrame, Vault},
    falling::{ConcretePowder, DragonEgg},
    fire::{Fire, SoulFire},
    flower_pot::FlowerPot,
    frogspawn::Frogspawn,
    frosted_ice::FrostedIce,
    hanging::{HangingMoss, ShelfMushroom, SporeBlossom},
    jukebox::Jukebox,
    leaves::Leaves,
    liquid::{BubbleColumn, Liquid},
    menu::{BrewingStand, Menu},
    portal::NetherPortal,
    potent_sulfur::PotentSulfur,
    pumpkin::Pumpkin,
    redstone_ore::RedstoneOre,
    respawn_anchor::RespawnAnchor,
    scaffolding::Scaffolding,
    sculk::{CalibratedSculkSensor, SculkCatalyst, SculkSensor, SculkShrieker},
    sea_pickle::SeaPickle,
    snow::{PowderSnow, SnowLayer},
    soil::{DirtPath, Farmland},
    speleothem::Speleothem,
    sponge::Sponge,
    test_block::TestBlock,
    tnt::Tnt,
    turtle_egg::TurtleEgg,
    waterlogged::{Barrier, Conduit, DriedGhast, Light, Waterlogged},
};
use super::{Behaviour, Plain};
use crate::{state::BlockState, world::World};

/// 这个家族里类到行为的分派。新版本多出来、还没移植的类是 `None`。
pub(super) fn of(class: BlockClass) -> Option<&'static dyn Behaviour> {
    use BlockClass as C;

    match class {
        C::AmethystClusterBlock => Some(&AmethystCluster),
        C::BarrierBlock => Some(&Barrier),
        C::HeavyCoreBlock
        | C::MangroveRootsBlock
        | C::WaterloggedTransparentBlock
        | C::WeatheringCopperGrateBlock => Some(&Waterlogged),
        C::BeaconBlock
        | C::CartographyTableBlock
        | C::CraftingTableBlock
        | C::EnchantingTableBlock
        | C::SmithingTableBlock => Some(&Menu),
        C::BrewingStandBlock => Some(&BrewingStand),
        C::BubbleColumnBlock => Some(&BubbleColumn),
        C::CakeBlock => Some(&Cake),
        C::CalibratedSculkSensorBlock => Some(&CalibratedSculkSensor),
        C::CandleBlock => Some(&Candle),
        C::CandleCakeBlock => Some(&CandleCake),
        C::CauldronBlock | C::LavaCauldronBlock | C::LayeredCauldronBlock => Some(&Cauldron),
        C::ComposterBlock => Some(&Composter),
        C::ConcretePowderBlock => Some(&ConcretePowder),
        C::ConduitBlock => Some(&Conduit),
        C::CopperBulbBlock | C::WeatheringCopperBulbBlock => Some(&CopperBulb),
        C::CopperGolemStatueBlock => Some(&CopperGolemStatue),
        C::CrafterBlock => Some(&Crafter),
        C::CreakingHeartBlock => Some(&CreakingHeart),
        C::DecoratedPotBlock => Some(&DecoratedPot),
        C::DragonEggBlock => Some(&DragonEgg),
        C::DriedGhastBlock => Some(&DriedGhast),
        C::EndPortalFrameBlock => Some(&EndPortalFrame),
        C::FarmlandBlock => Some(&Farmland),
        C::FireBlock => Some(&Fire),
        C::FlowerPotBlock => Some(&FlowerPot),
        C::FrogspawnBlock => Some(&Frogspawn),
        C::FrostedIceBlock => Some(&FrostedIce),
        C::HangingMossBlock => Some(&HangingMoss),
        C::JukeboxBlock => Some(&Jukebox),
        C::LeavesBlock
        | C::MangroveLeavesBlock
        | C::TintedParticleLeavesBlock
        | C::UntintedParticleLeavesBlock => Some(&Leaves),
        C::LightBlock => Some(&Light),
        C::LiquidBlock => Some(&Liquid),
        C::NetherPortalBlock => Some(&NetherPortal),
        C::PathBlock => Some(&DirtPath),
        C::PointedDripstoneBlock | C::SulfurSpikeBlock => Some(&Speleothem),
        C::PotentSulfurBlock => Some(&PotentSulfur),
        C::PowderSnowBlock => Some(&PowderSnow),
        C::PumpkinBlock => Some(&Pumpkin),
        C::RedStoneOreBlock => Some(&RedstoneOre),
        C::RespawnAnchorBlock => Some(&RespawnAnchor),
        C::ScaffoldingBlock => Some(&Scaffolding),
        C::SculkCatalystBlock => Some(&SculkCatalyst),
        C::SculkSensorBlock => Some(&SculkSensor),
        C::SculkShriekerBlock => Some(&SculkShrieker),
        C::SeaPickleBlock => Some(&SeaPickle),
        C::ShelfMushroomBlock => Some(&ShelfMushroom),
        C::SnowLayerBlock => Some(&SnowLayer),
        C::SoulFireBlock => Some(&SoulFire),
        C::SpongeBlock => Some(&Sponge),
        C::SporeBlossomBlock => Some(&SporeBlossom),
        C::TestBlock => Some(&TestBlock),
        C::TntBlock => Some(&Tnt),
        C::TurtleEggBlock => Some(&TurtleEgg),
        C::VaultBlock => Some(&Vault),
        C::WeatheringCopperGolemStatueBlock => Some(&WeatheringCopperGolemStatue),
        // 官方这些类不覆盖 `Behaviour` 的任何方法，只有形状或者和范围无关的东西。
        C::AirBlock
        | C::AmethystBlock
        | C::Block
        | C::BonemealableFeaturePlacerBlock
        | C::BuddingAmethystBlock
        | C::CryingObsidianBlock
        | C::DropExperienceBlock
        | C::EndGatewayBlock
        | C::EndPortalBlock
        | C::HalfTransparentBlock
        | C::HoneyBlock
        | C::IceBlock
        | C::InfestedBlock
        | C::MagmaBlock
        | C::MudBlock
        | C::NetherrackBlock
        | C::NyliumBlock
        | C::PoweredBlock
        | C::RootedDirtBlock
        | C::SculkBlock
        | C::SlimeBlock
        | C::SoulSandBlock
        | C::SpawnerBlock
        | C::StainedGlassBlock
        | C::StructureVoidBlock
        | C::TintedGlassBlock
        | C::TransparentBlock
        | C::TrialSpawnerBlock
        | C::WeatheringCopperFullBlock
        | C::WebBlock => Some(&Plain),
        // 官方的覆盖在这个世界里都走不到：下落和孵化的计划刻不在范围里，灯和靶子的状态只跟着信号变，
        // 主世界的水不会蒸发，`GameMasterBlock` 要权限，别的只碰方块实体的内容。
        C::BrushableBlock
        | C::ColoredFallingBlock
        | C::JigsawBlock
        | C::RedstoneLampBlock
        | C::SandBlock
        | C::SnifferEggBlock
        | C::StructureBlock
        | C::TargetBlock
        | C::TestInstanceBlock
        | C::WetSpongeBlock => Some(&Plain),
        _ => None,
    }
}

/// `FallingBlockEntity.fall` 在世界里留下的变化：这一格只剩流体。掉下去的实体不在范围里。
fn fall(world: &mut World, pos: BlockPos, state: BlockState) {
    world.set_block_and_update(pos, state.fluid().legacy_block());
}

#[cfg(test)]
mod test_support {
    use mc_bamboo_core::BlockPos;

    use crate::{
        BlockState, BlockTicks, TickPriority, World, run_scheduled_ticks, testing::TestLevel,
    };

    /// 直接放下一个方块，走完 `onPlace` 和全部连锁。
    pub(super) fn set(level: &mut TestLevel, pos: BlockPos, state: BlockState) {
        let mut ticks = BlockTicks::new();
        World::new(level, &mut ticks).set_block_and_update(pos, state);
    }

    /// 运行这一格的计划刻。
    pub(super) fn tick(level: &mut TestLevel, pos: BlockPos) {
        let mut ticks = BlockTicks::new();
        let block = level.get(pos).block_id();
        ticks.schedule(pos, block, 1, TickPriority::Normal);
        run_scheduled_ticks(level, &mut ticks, 1);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use crate::behaviour::{Family, family_of, is_ported};

    #[test]
    fn every_class_of_the_family_is_ported() {
        let unported: Vec<_> = blocks::BLOCKS
            .iter()
            .filter(|block| family_of(block.class) == Family::Misc && !is_ported(block.class))
            .map(|block| block.name)
            .collect();
        assert_eq!(unported, Vec::<&str>::new());
    }
}
