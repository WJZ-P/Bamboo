//! `SnowLayerBlock`、`PowderSnowBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, items, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_collision_face_full},
    },
    context::PlaceContext,
    level::Effect,
    state::{BlockState, LAYERS},
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `SnowLayerBlock`。
pub(super) struct SnowLayer;

/// `MAX_HEIGHT`。
const MAX_HEIGHT: i32 = 8;

impl Behaviour for SnowLayer {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = ctx.replaced_state(world);
        if state.is(block.default_state) {
            let layers = state.get(LAYERS) + 1;
            Some(state.with(LAYERS, layers.min(MAX_HEIGHT)))
        } else {
            Some(default_state(block))
        }
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, _world: &World) -> bool {
        let layers = state.get(LAYERS);
        if ctx.item != state.block().item || layers >= MAX_HEIGHT {
            layers == 1
        } else if ctx.replacing_clicked_on_block() {
            ctx.clicked_face() == Direction::Up
        } else {
            true
        }
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.block_state(below_pos);
        if below.is_tagged(block_tags::CANNOT_SUPPORT_SNOW_LAYER) {
            return false;
        }
        below.is_tagged(block_tags::SUPPORT_OVERRIDE_SNOW_LAYER)
            || is_collision_face_full(world, below_pos, Direction::Up)
            || below.is_same_block(state) && below.get(LAYERS) == MAX_HEIGHT
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if state.can_survive(world, pos) {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        } else {
            BlockState::AIR
        }
    }
}

/// `PowderSnowBlock`：范围里只有空桶舀起它这一件事。
pub(super) struct PowderSnow;

impl Behaviour for PowderSnow {
    /// 舀走以后官方发一个破坏事件，`source` 是空的，所以发起者也收得到。
    fn pickup_block(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _by_player: bool,
    ) -> i32 {
        world.set_block(pos, BlockState::AIR, UPDATE_ALL_IMMEDIATE);
        world.push_effect(Effect::Destroyed {
            pos,
            state,
            except_initiator: false,
        });
        items::POWDER_SNOW_BUCKET
    }
}
