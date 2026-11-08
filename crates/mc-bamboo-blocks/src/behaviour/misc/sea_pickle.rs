//! `SeaPickleBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_water_source, tick_water_if_waterlogged, vegetation_update_shape,
        },
    },
    context::PlaceContext,
    shape::{self, CollisionContext},
    state::{BlockState, PICKLES, SupportType, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// `SeaPickleBlock`。
pub(super) struct SeaPickle;

/// `MAX_PICKLES`。
const MAX_PICKLES: i32 = 4;
/// 骨粉撒海泡菜的那片菱形有 5 列宽。
const BONEMEAL_SPAN: i32 = 5;
/// 那片里每一格有六分之一的机会长一株。
const BONEMEAL_CHANCE: i32 = 6;

/// `mayPlaceOn`：顶上有碰撞面就行，不用是完整的面。
fn may_place_on(world: &World, pos: BlockPos) -> bool {
    let state = world.block_state(pos);
    let collision = state.collision_boxes(world, pos, CollisionContext::Empty);
    !shape::face_shape(&collision, Direction::Up).is_empty()
        || world.is_face_sturdy(pos, Direction::Up, SupportType::Full)
}

impl Behaviour for SeaPickle {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = ctx.replaced_state(world);
        if state.is(block.default_state) {
            let pickles = state.get(PICKLES) + 1;
            return Some(state.with(PICKLES, pickles.min(MAX_PICKLES)));
        }
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(default_state(block).with(WATERLOGGED, waterlogged))
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let adds_a_pickle = !ctx.is_secondary_use_active()
            && ctx.item == state.block().item
            && state.get(PICKLES) < MAX_PICKLES;
        adds_a_pickle || Plain.can_be_replaced(state, ctx, world)
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        may_place_on(world, pos.below())
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
        if !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        tick_water_if_waterlogged(state, world, pos);
        vegetation_update_shape(state, world, pos)
    }

    /// 活的海泡菜（泡在水里）放在珊瑚块上。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        state.get(WATERLOGGED)
            && world
                .block_state(pos.below())
                .is_tagged(block_tags::CORAL_BLOCKS)
    }

    /// 在周围菱形的一片珊瑚块上撒一些海泡菜，自己长满 4 个。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let mut z_span = 1;
        let mut z_offset = 0;
        for x in 0..BONEMEAL_SPAN {
            for z in 0..z_span {
                let column = BlockPos::new(pos.x - BONEMEAL_SPAN / 2 + x, 0, pos.z - z_offset + z);
                for y in pos.y - 1..=pos.y {
                    let at = BlockPos::new(column.x, y, column.z);
                    if at != pos && world.random().next_int(BONEMEAL_CHANCE) == 0 {
                        spread_pickle(world, at);
                    }
                }
            }
            if x < BONEMEAL_SPAN / 2 {
                z_span += 2;
                z_offset += 1;
            } else {
                z_span -= 2;
                z_offset -= 1;
            }
        }
        world.set_block(pos, state.with(PICKLES, MAX_PICKLES), UPDATE_CLIENTS);
    }
}

/// 珊瑚块上面的水里放一株随机个数的海泡菜。
fn spread_pickle(world: &mut World, pos: BlockPos) {
    if world.block_state(pos).is(blocks::WATER)
        && world
            .block_state(pos.below())
            .is_tagged(block_tags::CORAL_BLOCKS)
    {
        let pickles = world.random().next_int(MAX_PICKLES) + 1;
        let pickle = BlockState(blocks::SEA_PICKLE).with(PICKLES, pickles);
        world.set_block_and_update(pos, pickle);
    }
}
