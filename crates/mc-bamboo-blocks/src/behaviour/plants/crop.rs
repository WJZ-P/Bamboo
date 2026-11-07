//! `CropBlock`、`BeetrootBlock`、`CarrotBlock`、`PotatoBlock`、`TorchflowerCropBlock`、
//! `AttachedStemBlock`。`StemBlock` 只换了支撑条件，在 [`vegetation`](super::vegetation) 里。

use mc_bamboo_core::{BlockPos, Direction};

use super::vegetation::Vegetation;
use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, field_block, vegetation_update_shape},
    },
    state::{AGE_7, BlockState, HORIZONTAL_FACING},
    world::World,
};

/// `CropBlock.hasSufficientLight` 要求的亮度。
const CROP_MIN_BRIGHTNESS: i32 = 8;
/// `StemBlock.MAX_AGE`：瓜被摘走后，连着瓜的茎退回长满的茎。
const FULLY_GROWN_STEM: i32 = 7;

/// `CropBlock.hasSufficientLight`。
pub(super) fn has_sufficient_light(world: &World, pos: BlockPos) -> bool {
    world.raw_brightness(pos) >= CROP_MIN_BRIGHTNESS
}

/// `CropBlock`。
pub(super) struct Crop;

impl Behaviour for Crop {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_sufficient_light(world, pos) && Vegetation.can_survive(state, world, pos)
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

/// `AttachedStemBlock`：连着瓜的茎。
pub(super) struct AttachedStem;

impl Behaviour for AttachedStem {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        let block = state.block();
        let fruit = default_state(field_block(block, "fruit"));
        if !neighbor.is_same_block(fruit) && direction == state.get(HORIZONTAL_FACING) {
            return default_state(field_block(block, "stem")).with(AGE_7, FULLY_GROWN_STEM);
        }
        vegetation_update_shape(state, world, pos)
    }
}
