//! `FlowerBedBlock`、`LeafLitterBlock`，它们共用 `SegmentableBlock` 这个接口。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::vegetation::Vegetation;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, vegetation_update_shape},
    },
    context::PlaceContext,
    state::{BlockState, FLOWER_AMOUNT, HORIZONTAL_FACING, Property, SEGMENT_AMOUNT, SupportType},
    world::World,
};

/// `SegmentableBlock.MAX_SEGMENT`。
const MAX_SEGMENT: i32 = 4;

/// `SegmentableBlock.canBeReplaced`：拿着同样的物品时还能再加一片。
fn can_add_segment(state: BlockState, ctx: &PlaceContext, segment: Property<i32>) -> bool {
    !ctx.is_secondary_use_active()
        && ctx.item == state.block().item
        && state.get(segment) < MAX_SEGMENT
}

/// `SegmentableBlock.getStateForPlacement`。
fn segment_placement(
    block: &'static Block,
    ctx: &PlaceContext,
    world: &World,
    segment: Property<i32>,
) -> BlockState {
    let default = default_state(block);
    let old = world.block_state(ctx.clicked_pos());
    if old.is_same_block(default) {
        old.with(segment, (old.get(segment) + 1).min(MAX_SEGMENT))
    } else {
        default.with(HORIZONTAL_FACING, ctx.horizontal_direction().opposite())
    }
}

/// `FlowerBedBlock`：粉红色花簇和野花。
pub(super) struct FlowerBed;

impl Behaviour for FlowerBed {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Some(segment_placement(block, ctx, world, FLOWER_AMOUNT))
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        can_add_segment(state, ctx, FLOWER_AMOUNT) || Plain.can_be_replaced(state, ctx, world)
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
        vegetation_update_shape(state, world, pos)
    }
}

/// `LeafLitterBlock`：落叶。
pub(super) struct LeafLitter;

impl Behaviour for LeafLitter {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Some(segment_placement(block, ctx, world, SEGMENT_AMOUNT))
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        can_add_segment(state, ctx, SEGMENT_AMOUNT) || Plain.can_be_replaced(state, ctx, world)
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.is_face_sturdy(pos.below(), Direction::Up, SupportType::Full)
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
