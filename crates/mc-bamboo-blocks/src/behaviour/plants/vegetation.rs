//! `VegetationBlock` 和只换了支撑条件的子类：花、草、树苗、灌木、菌类、下界疣、荷叶、茎，
//! 以及多覆盖了几个方法的 `MushroomBlock`、`MangrovePropaguleBlock`、`SeagrassBlock`、
//! `SweetBerryBushBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    Block, BlockClass, items,
    registries::fluid,
    tags::{block_tags, fluid_tags},
};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, field_tag, is_full_water, is_water_source, may_place_vegetation_on,
            tick_water_if_waterlogged, vegetation_update_shape,
        },
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{AGE_3, AGE_4, BlockState, FluidState, HANGING, SupportType, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// `MushroomBlock.canSurvive` 要求亮度低于这个值。
const MUSHROOM_MAX_BRIGHTNESS: i32 = 13;
/// `MangrovePropaguleBlock.MAX_AGE`：放下时就是长成的样子。
const FULLY_GROWN_PROPAGULE: i32 = 4;
/// `SweetBerryBushBlock.MAX_AGE`。
const RIPE_BUSH_AGE: i32 = 3;
/// 摘完果子后退回的 `age`，比它大才有果子。
const PICKED_BUSH_AGE: i32 = 1;

/// `mayPlaceOn`：`plant` 能不能长在 `support_pos` 这一格的 `support` 上。
///
/// 这是官方最常被子类覆盖的方法，所有覆盖都列在这里，只换了它的子类不用写结构体。
pub(super) fn may_place_on(
    plant: BlockState,
    support: BlockState,
    world: &World,
    support_pos: BlockPos,
) -> bool {
    use BlockClass as C;

    let block = plant.block();
    match plant.class() {
        C::AttachedStemBlock | C::NetherFungusBlock | C::NetherRootsBlock => {
            support.is_tagged(field_tag(block, "supportBlocks"))
        }
        C::AzaleaBlock => support.is_tagged(block_tags::SUPPORTS_AZALEA),
        C::BeetrootBlock
        | C::CarrotBlock
        | C::CropBlock
        | C::PitcherCropBlock
        | C::PotatoBlock
        | C::TorchflowerCropBlock => support.is_tagged(block_tags::SUPPORTS_CROPS),
        C::CactusFlowerBlock => {
            support.is_tagged(block_tags::SUPPORT_OVERRIDE_CACTUS_FLOWER)
                || world.is_face_sturdy(support_pos, Direction::Up, SupportType::Center)
        }
        C::DryVegetationBlock | C::ShortDryGrassBlock | C::TallDryGrassBlock => {
            support.is_tagged(block_tags::SUPPORTS_DRY_VEGETATION)
        }
        C::LilyPadBlock => lily_pad_may_place_on(support, world, support_pos),
        C::MangrovePropaguleBlock => support.is_tagged(block_tags::SUPPORTS_MANGROVE_PROPAGULE),
        C::MushroomBlock => support.is_solid_render(),
        C::NetherSproutsBlock => support.is_tagged(block_tags::SUPPORTS_NETHER_SPROUTS),
        C::NetherWartBlock => support.is_tagged(block_tags::SUPPORTS_NETHER_WART),
        C::SeagrassBlock | C::TallSeagrassBlock => {
            world.is_face_sturdy(support_pos, Direction::Up, SupportType::Full)
                && !support.is_tagged(block_tags::CANNOT_SUPPORT_SEAGRASS)
        }
        C::SmallDripleafBlock => {
            support.is_tagged(block_tags::SUPPORTS_SMALL_DRIPLEAF)
                || world.fluid_state(support_pos.above()).is(fluid::WATER)
                    && may_place_vegetation_on(support)
        }
        C::StemBlock => support.is_tagged(field_tag(block, "stemSupportBlocks")),
        C::WitherRoseBlock => support.is_tagged(block_tags::SUPPORTS_WITHER_ROSE),
        _ => may_place_vegetation_on(support),
    }
}

/// `LilyPadBlock.mayPlaceOn`：下面是水或者冰，上方不能有流体。
fn lily_pad_may_place_on(support: BlockState, world: &World, support_pos: BlockPos) -> bool {
    let on_fluid = world
        .fluid_state(support_pos)
        .is_tagged(fluid_tags::SUPPORTS_LILY_PAD);
    (on_fluid || support.is_tagged(block_tags::SUPPORTS_LILY_PAD))
        && world.fluid_state(support_pos.above()).is_empty()
}

/// `VegetationBlock`。
pub(super) struct Vegetation;

impl Behaviour for Vegetation {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();
        may_place_on(state, world.block_state(below), world, below)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        vegetation_update_shape(state, world, pos)
    }
}

/// `MushroomBlock`：亮的地方只能长在菌丝这类方块上。
pub(super) struct Mushroom;

impl Behaviour for Mushroom {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();
        let support = world.block_state(below);
        support.is_tagged(block_tags::OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT)
            || world.raw_brightness(pos) < MUSHROOM_MAX_BRIGHTNESS
                && may_place_on(state, support, world, below)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        vegetation_update_shape(state, world, pos)
    }
}

/// `MangrovePropaguleBlock`：红树胎生苗，可以挂在叶子下面。
pub(super) struct MangrovePropagule;

impl Behaviour for MangrovePropagule {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        let state = default_state(block)
            .with(WATERLOGGED, waterlogged)
            .with(AGE_4, FULLY_GROWN_PROPAGULE);
        Some(state)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        if state.get(HANGING) {
            world
                .block_state(pos.above())
                .is_tagged(block_tags::SUPPORTS_HANGING_MANGROVE_PROPAGULE)
        } else {
            Vegetation.can_survive(state, world, pos)
        }
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        tick_water_if_waterlogged(state, world, pos);
        // 官方先单独判定了上面一格的更新，`VegetationBlock.updateShape` 判定的是同一个
        // `canSurvive`，结果一样。
        vegetation_update_shape(state, world, pos)
    }
}

/// `SeagrassBlock`。
pub(super) struct Seagrass;

impl Behaviour for Seagrass {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        is_full_water(world.fluid_state(ctx.clicked_pos())).then(|| default_state(block))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        let updated = vegetation_update_shape(state, world, pos);
        if !updated.is_air() {
            world.schedule_fluid_tick(pos, fluid::WATER);
        }
        updated
    }

    /// 海草本来就泡在水里，装不了别的。
    fn can_place_liquid(
        &self,
        _state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _fluid: i32,
        _by_player: bool,
    ) -> bool {
        false
    }

    fn place_liquid(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _fluid: FluidState,
    ) -> bool {
        false
    }
}

/// `SweetBerryBushBlock`。
pub(super) struct SweetBerryBush;

impl Behaviour for SweetBerryBush {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        vegetation_update_shape(state, world, pos)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        // 没长满时拿着骨粉不摘果子，让点击落到骨粉自己的 `useOn`。
        if state.get(AGE_3) != RIPE_BUSH_AGE && ctx.item == items::BONE_MEAL {
            InteractionResult::Pass
        } else {
            Plain.use_item_on(state, world, ctx)
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(AGE_3) <= PICKED_BUSH_AGE {
            return Plain.use_without_item(state, world, ctx);
        }
        let picked = state.with(AGE_3, PICKED_BUSH_AGE);
        world.set_block(ctx.clicked_pos(), picked, UPDATE_CLIENTS);
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::testing::{TestLevel, actor_holding, use_in_air};

    /// `PlaceOnWaterBlockItem` 走 `Item.use`，oracle 没有这种题。
    #[test]
    fn a_lily_pad_does_not_go_on_solid_ground() {
        let mut level = TestLevel::new();
        let ground = BlockPos::new(0, 63, 3);
        level.set(ground, BlockState(blocks::STONE));
        let mut actor = actor_holding(items::LILY_PAD);
        actor.position = Vec3::new(0.5, 64.0, 3.5);
        actor.pitch = 90.0;
        let outcome = use_in_air(&mut level, &actor);
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(ground.above()).is_air());
    }
}
