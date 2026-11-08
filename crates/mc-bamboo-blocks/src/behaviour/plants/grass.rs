//! `BushBlock`、`FireflyBushBlock`、`ShortDryGrassBlock`、`TallDryGrassBlock`、`TallGrassBlock`：
//! 支撑和 `VegetationBlock` 一样，只多了骨粉。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockClass, blocks};

use super::vegetation::Vegetation;
use crate::{
    behaviour::{Behaviour, common::vegetation_update_shape},
    state::{BlockState, DOUBLE_BLOCK_HALF, DoubleBlockHalf, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// `BonemealableBlock.getSpreadableNeighbourPos`：按顺序第一个空着、`plant` 又放得住的水平邻居。
fn spreadable_neighbour(
    world: &World,
    pos: BlockPos,
    plant: BlockState,
    directions: [Direction; 4],
) -> Option<BlockPos> {
    directions
        .into_iter()
        .map(|direction| pos.relative(direction))
        .find(|&neighbour| world.is_empty_block(neighbour) && plant.can_survive(world, neighbour))
}

/// 骨粉在旁边长出来的植物：灌木和萤火虫灌木长一丛一样的，高的枯草长一丛矮的。
fn spread_plant(state: BlockState) -> BlockState {
    if state.class() == BlockClass::TallDryGrassBlock {
        BlockState(blocks::SHORT_DRY_GRASS)
    } else {
        state.default_state()
    }
}

/// `TallGrassBlock.getGrownBlock`：草长成高草，蕨长成大型蕨。
fn grown_tall_grass(state: BlockState) -> BlockState {
    if state.is(blocks::SHORT_GRASS) {
        BlockState(blocks::TALL_GRASS)
    } else {
        BlockState(blocks::LARGE_FERN)
    }
}

/// `TallGrassBlock.isValidBonemealTarget`。
fn tall_grass_has_room(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let above = pos.above();
    grown_tall_grass(state).can_survive(world, pos)
        && world.is_empty_block(above)
        && world.is_inside_build_height(above.y)
}

/// `DoublePlantBlock.placeAt`：两半各自按那一格的水决定含不含水。
fn place_double_plant(world: &mut World, plant: BlockState, pos: BlockPos) {
    for (at, half) in [
        (pos, DoubleBlockHalf::Lower),
        (pos.above(), DoubleBlockHalf::Upper),
    ] {
        let part = plant.with(DOUBLE_BLOCK_HALF, half);
        let part = part
            .try_with(WATERLOGGED, world.is_water_at(at))
            .unwrap_or(part);
        world.set_block(at, part, UPDATE_CLIENTS);
    }
}

/// 这个文件里的五个类。
pub(super) struct Grass;

impl Behaviour for Grass {
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

    /// 长的那两种总能长；往旁边长的按 `Direction.Plane.HORIZONTAL` 的顺序找空位。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        match state.class() {
            BlockClass::ShortDryGrassBlock => true,
            BlockClass::TallGrassBlock => tall_grass_has_room(state, world, pos),
            _ => spreadable_neighbour(world, pos, spread_plant(state), Direction::HORIZONTAL)
                .is_some(),
        }
    }

    /// 往旁边长的先把四个方向洗一遍再找，`findSpreadableNeighbourPos`。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        match state.class() {
            BlockClass::ShortDryGrassBlock => {
                world.set_block_and_update(pos, BlockState(blocks::TALL_DRY_GRASS));
            }
            BlockClass::TallGrassBlock => {
                place_double_plant(world, grown_tall_grass(state), pos);
            }
            _ => {
                let plant = spread_plant(state);
                let directions = world.random().shuffled_horizontal();
                if let Some(neighbour) = spreadable_neighbour(world, pos, plant, directions) {
                    world.set_block_and_update(neighbour, plant);
                }
            }
        }
    }
}
