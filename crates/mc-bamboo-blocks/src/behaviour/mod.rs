//! 方块行为，对应官方 `BlockBehaviour` 和 `Block` 的可覆盖方法。
//!
//! Java 的继承在这里用组合表达：
//!
//! - 每个官方类是一个实现 [`Behaviour`] 的结构体，放在所属家族的目录里。
//! - 子类调用 `super.xxx` 时，直接调用父类结构体的方法：`Plain.update_shape(...)`。
//! - 官方代码里的虚调用（`this.canSurvive`、`state.canSurvive`）写成 `state.can_survive(...)`，
//!   它按状态的类重新分派，所以子类的覆盖能生效。不要写成 `self.can_survive`。
//! - 行为结构体没有字段，官方的构造参数按方块查：`state.block().field("deadBlock")`，
//!   或者用 [`common`] 里带类型的 `field_*` 函数。
//!
//! 分派是 [`BlockClass`] 上的 `match`，新版本多一个类时这里编译失败，这正是想要的。
//! 每个类属于且只属于一个家族，家族用自己的 `of` 把类分给行为结构体，官方什么都不覆盖的类返回
//! `Some(&Plain)`。现在全部类都移植了，新版本多出来的类加进家族后先返回 `None`，
//! oracle 靠这个区分“还没移植”和“移植错了”。

pub mod attached;
pub mod building;
pub mod common;
pub mod machines;
pub mod misc;
pub mod plants;

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockClass};

use crate::{
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    level::Effect,
    shape::{self, Boxes, CollisionContext},
    state::BlockState,
    world::World,
};

/// 一个官方方块类的行为。每个方法都有和 `BlockBehaviour` 相同的默认实现。
pub trait Behaviour: Sync {
    /// `getStateForPlacement`。`None` 表示这里放不了。
    fn state_for_placement(
        &self,
        block: &'static Block,
        _ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(BlockState(block.default_state))
    }

    /// `getStateForPlacement` 里写世界的部分，在 `state_for_placement` 之前调用。
    /// 官方只有 `CoralBlock` 在这里排计划刻，放不下时这个刻也还在。
    fn before_placement(&self, _block: &'static Block, _ctx: &PlaceContext, _world: &mut World) {}

    /// `canBeReplaced(state, context)`。
    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, _world: &World) -> bool {
        state.is_replaceable() && (ctx.hand_is_empty() || ctx.item != state.block().item)
    }

    /// `canSurvive`。
    fn can_survive(&self, _state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        true
    }

    /// `updateShape`：`neighbor_pos` 的方块变成了 `neighbor`，返回这一格的新状态。
    /// 返回空气时这一格被破坏。这里只能读世界和排计划刻，写世界要等连锁。
    fn update_shape(
        &self,
        state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        state
    }

    /// `updateIndirectNeighbourShapes`。
    fn update_indirect_neighbour_shapes(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _flags: u32,
        _limit: u32,
    ) {
    }

    /// `onPlace`。
    fn on_place(&self, _state: BlockState, _world: &mut World, _pos: BlockPos, _old: BlockState) {}

    /// `affectNeighborsAfterRemoval`。
    fn affect_neighbors_after_removal(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
    ) {
    }

    /// `neighborChanged`。`source` 是引起更新的方块，官方的 `block` 参数。
    fn neighbor_changed(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _source: &'static Block,
    ) {
    }

    /// `setPlacedBy`。
    fn set_placed_by(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _placer: &Actor,
    ) {
    }

    /// `playerWillDestroy`：返回破坏前的状态，官方后面的掉落物计算要用它，这里只用来和官方对照。
    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _player: &Actor,
    ) -> BlockState {
        state.spawn_destroy_by_entity_particles(world, pos, true);
        state
    }

    /// `useItemOn`。
    fn use_item_on(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::TryEmptyHandInteraction
    }

    /// `useWithoutItem`。
    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// `tick`：计划刻到期。
    fn tick(&self, _state: BlockState, _world: &mut World, _pos: BlockPos) {}

    /// `getCollisionShape`。默认读生成的表。
    fn collision_boxes(
        &self,
        state: BlockState,
        _world: &World,
        pos: BlockPos,
        context: CollisionContext,
    ) -> Boxes {
        shape::collision_boxes(state, pos, context)
    }

    /// `spawnDestroyParticles`：`Level.destroyBlock` 里的破坏效果。
    fn spawn_destroy_particles(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        state.spawn_destroy_by_entity_particles(world, pos, false);
    }

    /// `spawnDestroyByEntityParticles`。`by_player` 对应官方的 `entity` 参数不为空。
    fn spawn_destroy_by_entity_particles(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        by_player: bool,
    ) {
        world.push_effect(Effect::Destroyed {
            pos,
            state,
            except_initiator: by_player,
        });
    }

    /// `HangingSignItem.canPlace` 里对 `WallHangingSignBlock` 多做的判定，别的类都是 `true`。
    fn can_place_from_hanging_sign_item(
        &self,
        _state: BlockState,
        _world: &World,
        _pos: BlockPos,
    ) -> bool {
        true
    }
}

/// `BlockBehaviour` 本身，全部用默认实现。`super.xxx` 走到底时调用它。
pub struct Plain;

impl Behaviour for Plain {}

/// 分工的五个家族，每个家族一个目录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    Building,
    Attached,
    Plants,
    Machines,
    Misc,
}

impl Family {
    pub const ALL: [Self; 5] = [
        Self::Building,
        Self::Attached,
        Self::Plants,
        Self::Machines,
        Self::Misc,
    ];

    /// 家族的目录名。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Attached => "attached",
            Self::Plants => "plants",
            Self::Machines => "machines",
            Self::Misc => "misc",
        }
    }
}

/// 官方类属于哪个家族。
pub const fn family_of(class: BlockClass) -> Family {
    use BlockClass as C;

    match class {
        C::BedBlock
        | C::CarpetBlock
        | C::ChainBlock
        | C::DoorBlock
        | C::FenceBlock
        | C::FenceGateBlock
        | C::GlazedTerracottaBlock
        | C::GrassBlock
        | C::HayBlock
        | C::HugeMushroomBlock
        | C::InfestedRotatedPillarBlock
        | C::IronBarsBlock
        | C::MossyCarpetBlock
        | C::MyceliumBlock
        | C::RotatedPillarBlock
        | C::SlabBlock
        | C::SnowyBlock
        | C::StainedGlassPaneBlock
        | C::StairBlock
        | C::StrawBedBlock
        | C::TrapDoorBlock
        | C::WallBlock
        | C::WeatheringCopperBarsBlock
        | C::WeatheringCopperChainBlock
        | C::WeatheringCopperDoorBlock
        | C::WeatheringCopperSlabBlock
        | C::WeatheringCopperStairBlock
        | C::WeatheringCopperTrapDoorBlock
        | C::WoolCarpetBlock => Family::Building,
        C::BannerBlock
        | C::BellBlock
        | C::ButtonBlock
        | C::CeilingHangingSignBlock
        | C::ChiseledBookShelfBlock
        | C::EndRodBlock
        | C::LadderBlock
        | C::LanternBlock
        | C::LeverBlock
        | C::LightningRodBlock
        | C::PiglinWallSkullBlock
        | C::PlayerHeadBlock
        | C::PlayerWallHeadBlock
        | C::PressurePlateBlock
        | C::ShelfBlock
        | C::SkullBlock
        | C::StandingSignBlock
        | C::TorchBlock
        | C::TripWireHookBlock
        | C::WallBannerBlock
        | C::WallHangingSignBlock
        | C::WallSignBlock
        | C::WallSkullBlock
        | C::WallTorchBlock
        | C::WeatheringLanternBlock
        | C::WeatheringLightningRodBlock
        | C::WeightedPressurePlateBlock
        | C::WitherSkullBlock
        | C::WitherWallSkullBlock => Family::Attached,
        C::AttachedStemBlock
        | C::AzaleaBlock
        | C::BambooSaplingBlock
        | C::BambooStalkBlock
        | C::BaseCoralFanBlock
        | C::BaseCoralPlantBlock
        | C::BaseCoralWallFanBlock
        | C::BeetrootBlock
        | C::BigDripleafBlock
        | C::BigDripleafStemBlock
        | C::BushBlock
        | C::CactusBlock
        | C::CactusFlowerBlock
        | C::CarrotBlock
        | C::CaveVinesBlock
        | C::CaveVinesPlantBlock
        | C::ChorusFlowerBlock
        | C::ChorusPlantBlock
        | C::CocoaBlock
        | C::CoralBlock
        | C::CoralFanBlock
        | C::CoralPlantBlock
        | C::CoralWallFanBlock
        | C::CropBlock
        | C::DoublePlantBlock
        | C::DryVegetationBlock
        | C::EyeblossomBlock
        | C::FireflyBushBlock
        | C::FlowerBedBlock
        | C::FlowerBlock
        | C::GlowLichenBlock
        | C::HangingRootsBlock
        | C::KelpBlock
        | C::KelpPlantBlock
        | C::LeafLitterBlock
        | C::LilyPadBlock
        | C::MangrovePropaguleBlock
        | C::MultifaceBlock
        | C::MushroomBlock
        | C::NetherFungusBlock
        | C::NetherRootsBlock
        | C::NetherSproutsBlock
        | C::NetherWartBlock
        | C::PitcherCropBlock
        | C::PotatoBlock
        | C::SaplingBlock
        | C::SculkVeinBlock
        | C::SeagrassBlock
        | C::ShortDryGrassBlock
        | C::SmallDripleafBlock
        | C::StemBlock
        | C::SugarCaneBlock
        | C::SweetBerryBushBlock
        | C::TallDryGrassBlock
        | C::TallFlowerBlock
        | C::TallGrassBlock
        | C::TallSeagrassBlock
        | C::TorchflowerCropBlock
        | C::TwistingVinesBlock
        | C::TwistingVinesPlantBlock
        | C::VineBlock
        | C::WeepingVinesBlock
        | C::WeepingVinesPlantBlock
        | C::WitherRoseBlock => Family::Plants,
        C::AnvilBlock
        | C::BarrelBlock
        | C::BeehiveBlock
        | C::BlastFurnaceBlock
        | C::CampfireBlock
        | C::CarvedPumpkinBlock
        | C::ChestBlock
        | C::CommandBlock
        | C::ComparatorBlock
        | C::CopperChestBlock
        | C::DaylightDetectorBlock
        | C::DetectorRailBlock
        | C::DispenserBlock
        | C::DropperBlock
        | C::EnderChestBlock
        | C::FurnaceBlock
        | C::GrindstoneBlock
        | C::HopperBlock
        | C::LecternBlock
        | C::LoomBlock
        | C::MovingPistonBlock
        | C::NoteBlock
        | C::ObserverBlock
        | C::PistonBaseBlock
        | C::PistonHeadBlock
        | C::PoweredRailBlock
        | C::RailBlock
        | C::RedstoneTorchBlock
        | C::RedstoneWallTorchBlock
        | C::RedstoneWireBlock
        | C::RepeaterBlock
        | C::ShulkerBoxBlock
        | C::SmokerBlock
        | C::StonecutterBlock
        | C::TrappedChestBlock
        | C::TripWireBlock
        | C::WeatheringCopperChestBlock => Family::Machines,
        C::AirBlock
        | C::AmethystBlock
        | C::AmethystClusterBlock
        | C::BarrierBlock
        | C::BeaconBlock
        | C::Block
        | C::BonemealableFeaturePlacerBlock
        | C::BrewingStandBlock
        | C::BrushableBlock
        | C::BubbleColumnBlock
        | C::BuddingAmethystBlock
        | C::CakeBlock
        | C::CalibratedSculkSensorBlock
        | C::CandleBlock
        | C::CandleCakeBlock
        | C::CartographyTableBlock
        | C::CauldronBlock
        | C::ColoredFallingBlock
        | C::ComposterBlock
        | C::ConcretePowderBlock
        | C::ConduitBlock
        | C::CopperBulbBlock
        | C::CopperGolemStatueBlock
        | C::CrafterBlock
        | C::CraftingTableBlock
        | C::CreakingHeartBlock
        | C::CryingObsidianBlock
        | C::DecoratedPotBlock
        | C::DragonEggBlock
        | C::DriedGhastBlock
        | C::DropExperienceBlock
        | C::EnchantingTableBlock
        | C::EndGatewayBlock
        | C::EndPortalBlock
        | C::EndPortalFrameBlock
        | C::FarmlandBlock
        | C::FireBlock
        | C::FlowerPotBlock
        | C::FrogspawnBlock
        | C::FrostedIceBlock
        | C::HalfTransparentBlock
        | C::HangingMossBlock
        | C::HeavyCoreBlock
        | C::HoneyBlock
        | C::IceBlock
        | C::InfestedBlock
        | C::JigsawBlock
        | C::JukeboxBlock
        | C::LavaCauldronBlock
        | C::LayeredCauldronBlock
        | C::LeavesBlock
        | C::LightBlock
        | C::LiquidBlock
        | C::MagmaBlock
        | C::MangroveLeavesBlock
        | C::MangroveRootsBlock
        | C::MudBlock
        | C::NetherPortalBlock
        | C::NetherrackBlock
        | C::NyliumBlock
        | C::PathBlock
        | C::PointedDripstoneBlock
        | C::PotentSulfurBlock
        | C::PowderSnowBlock
        | C::PoweredBlock
        | C::PumpkinBlock
        | C::RedStoneOreBlock
        | C::RedstoneLampBlock
        | C::RespawnAnchorBlock
        | C::RootedDirtBlock
        | C::SandBlock
        | C::ScaffoldingBlock
        | C::SculkBlock
        | C::SculkCatalystBlock
        | C::SculkSensorBlock
        | C::SculkShriekerBlock
        | C::SeaPickleBlock
        | C::ShelfMushroomBlock
        | C::SlimeBlock
        | C::SmithingTableBlock
        | C::SnifferEggBlock
        | C::SnowLayerBlock
        | C::SoulFireBlock
        | C::SoulSandBlock
        | C::SpawnerBlock
        | C::SpongeBlock
        | C::SporeBlossomBlock
        | C::StainedGlassBlock
        | C::StructureBlock
        | C::StructureVoidBlock
        | C::SulfurSpikeBlock
        | C::TargetBlock
        | C::TestBlock
        | C::TestInstanceBlock
        | C::TintedGlassBlock
        | C::TintedParticleLeavesBlock
        | C::TntBlock
        | C::TransparentBlock
        | C::TrialSpawnerBlock
        | C::TurtleEggBlock
        | C::UntintedParticleLeavesBlock
        | C::VaultBlock
        | C::WaterloggedTransparentBlock
        | C::WeatheringCopperBulbBlock
        | C::WeatheringCopperFullBlock
        | C::WeatheringCopperGolemStatueBlock
        | C::WeatheringCopperGrateBlock
        | C::WebBlock
        | C::WetSpongeBlock => Family::Misc,
    }
}

/// 已经移植的类的行为，还没移植时是 `None`。
fn ported_behaviour(class: BlockClass) -> Option<&'static dyn Behaviour> {
    match family_of(class) {
        Family::Building => building::of(class),
        Family::Attached => attached::of(class),
        Family::Plants => plants::of(class),
        Family::Machines => machines::of(class),
        Family::Misc => misc::of(class),
    }
}

/// [`BlockClass`] 到行为的分派表。还没移植的类先走官方默认。
pub fn behaviour_of(class: BlockClass) -> &'static dyn Behaviour {
    ported_behaviour(class).unwrap_or(&Plain)
}

/// 这个类已经移植了，或者官方本来就什么都不覆盖。
pub fn is_ported(class: BlockClass) -> bool {
    ported_behaviour(class).is_some()
}

/// `block.getStateForPlacement(context)`。
pub fn state_for_placement(
    block: &'static Block,
    ctx: &PlaceContext,
    world: &World,
) -> Option<BlockState> {
    behaviour_of(block.class).state_for_placement(block, ctx, world)
}

/// 按状态的类分派，相当于官方在 `BlockState` 上调用同名方法。
impl BlockState {
    /// 这个状态的行为。
    pub fn behaviour(self) -> &'static dyn Behaviour {
        behaviour_of(self.class())
    }

    /// `canBeReplaced(context)`。
    pub fn can_be_replaced(self, ctx: &PlaceContext, world: &World) -> bool {
        self.behaviour().can_be_replaced(self, ctx, world)
    }

    /// `canSurvive`。
    pub fn can_survive(self, world: &World, pos: BlockPos) -> bool {
        self.behaviour().can_survive(self, world, pos)
    }

    /// `updateShape`。
    pub fn update_shape(
        self,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        self.behaviour()
            .update_shape(self, world, pos, direction, neighbor_pos, neighbor)
    }

    /// `updateIndirectNeighbourShapes`。
    pub fn update_indirect_neighbour_shapes(
        self,
        world: &mut World,
        pos: BlockPos,
        flags: u32,
        limit: u32,
    ) {
        self.behaviour()
            .update_indirect_neighbour_shapes(self, world, pos, flags, limit);
    }

    /// `onPlace`。
    pub fn on_place(self, world: &mut World, pos: BlockPos, old: BlockState) {
        self.behaviour().on_place(self, world, pos, old);
    }

    /// `affectNeighborsAfterRemoval`。
    pub fn affect_neighbors_after_removal(self, world: &mut World, pos: BlockPos) {
        self.behaviour()
            .affect_neighbors_after_removal(self, world, pos);
    }

    /// `handleNeighborChanged`。
    pub fn neighbor_changed(self, world: &mut World, pos: BlockPos, source: &'static Block) {
        self.behaviour().neighbor_changed(self, world, pos, source);
    }

    /// `setPlacedBy`。
    pub fn set_placed_by(self, world: &mut World, pos: BlockPos, placer: &Actor) {
        self.behaviour().set_placed_by(self, world, pos, placer);
    }

    /// `playerWillDestroy`。
    pub fn player_will_destroy(
        self,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        self.behaviour()
            .player_will_destroy(self, world, pos, player)
    }

    /// `useItemOn`。
    pub fn use_item_on(self, world: &mut World, ctx: &UseContext) -> InteractionResult {
        self.behaviour().use_item_on(self, world, ctx)
    }

    /// `useWithoutItem`。
    pub fn use_without_item(self, world: &mut World, ctx: &UseContext) -> InteractionResult {
        self.behaviour().use_without_item(self, world, ctx)
    }

    /// `tick`。
    pub fn tick(self, world: &mut World, pos: BlockPos) {
        self.behaviour().tick(self, world, pos);
    }

    /// `getCollisionShape`。
    pub fn collision_boxes(self, world: &World, pos: BlockPos, context: CollisionContext) -> Boxes {
        self.behaviour().collision_boxes(self, world, pos, context)
    }

    /// `spawnDestroyParticles`。
    pub fn spawn_destroy_particles(self, world: &mut World, pos: BlockPos) {
        self.behaviour().spawn_destroy_particles(self, world, pos);
    }

    /// `spawnDestroyByEntityParticles`。
    pub fn spawn_destroy_by_entity_particles(
        self,
        world: &mut World,
        pos: BlockPos,
        by_player: bool,
    ) {
        self.behaviour()
            .spawn_destroy_by_entity_particles(self, world, pos, by_player);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    /// 分派表本身不能 panic，全部类都能取到行为。
    #[test]
    fn every_block_has_a_behaviour() {
        for block in blocks::BLOCKS {
            let _ = behaviour_of(block.class);
        }
    }

    #[test]
    fn families_follow_the_division_of_work() {
        assert_eq!(family_of(BlockClass::SlabBlock), Family::Building);
        assert_eq!(family_of(BlockClass::TorchBlock), Family::Attached);
        assert_eq!(family_of(BlockClass::SugarCaneBlock), Family::Plants);
        assert_eq!(family_of(BlockClass::ChestBlock), Family::Machines);
        assert_eq!(family_of(BlockClass::Block), Family::Misc);
    }

    /// 官方什么都不覆盖的类不用写结构体，但是要算移植好了，否则 oracle 会一直把它们算成待办。
    #[test]
    fn classes_without_overrides_count_as_ported() {
        assert!(is_ported(BlockClass::Block));
        assert!(is_ported(BlockClass::AirBlock));
        assert!(is_ported(BlockClass::RotatedPillarBlock));
    }
}
