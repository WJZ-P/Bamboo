//! `SpeleothemBlock` 和子类 `PointedDripstoneBlock`、`SulfurSpikeBlock`，还有炼药锅接水要用的
//! 查找函数。生长在随机刻里，不在范围里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, registries::fluid, tags::block_tags};

use super::fall;
use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    fluid::Fluid,
    shape::{self, Aabb, BooleanOp, CollisionContext},
    state::{
        BlockState, SPELEOTHEM_THICKNESS, SpeleothemThickness, SupportType, VERTICAL_DIRECTION,
        WATERLOGGED,
    },
    world::{UPDATE_LIMIT, World},
};

/// `SpeleothemBlock`。
pub(super) struct Speleothem;

/// 钟乳石失去支撑后等多久才掉下来，`DELAY_BEFORE_FALLING`。
const DELAY_BEFORE_FALLING: i32 = 2;
/// `MAX_SEARCH_LENGTH_BETWEEN_STALACTITE_TIP_AND_CAULDRON`，也是 `findRootBlock` 的上限。
const MAX_SEARCH_LENGTH: i32 = 11;
/// `REQUIRED_SPACE_TO_DRIP_THROUGH_NON_SOLID_BLOCK`：`Block.column(4.0, 0.0, 16.0)`。
const DRIP_SPACE: &[Aabb] = &[Aabb::new(
    6.0 / 16.0,
    0.0,
    6.0 / 16.0,
    10.0 / 16.0,
    1.0,
    10.0 / 16.0,
)];

/// `isSpeleothemWithDirection`。
fn points(state: BlockState, tip: Direction) -> bool {
    state.is_tagged(block_tags::SPELEOTHEMS) && state.get(VERTICAL_DIRECTION) == tip
}

/// `isStalactite`。
fn is_stalactite(state: BlockState) -> bool {
    points(state, Direction::Down)
}

/// `isStalagmite`。
fn is_stalagmite(state: BlockState) -> bool {
    points(state, Direction::Up)
}

/// `isTip`，`includeMergedTip` 是真。
fn is_tip_or_merged(state: BlockState) -> bool {
    state.is_tagged(block_tags::SPELEOTHEMS)
        && matches!(
            state.get(SPELEOTHEM_THICKNESS),
            SpeleothemThickness::Tip | SpeleothemThickness::TipMerge
        )
}

/// `isFreeHangingStalactite`。
fn is_free_hanging_stalactite(state: BlockState) -> bool {
    is_stalactite(state)
        && state.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::Tip
        && !state.get(WATERLOGGED)
}

/// `isValidSpeleothemPlacement`：`own` 是这个方块的任意一个状态。
fn is_valid_placement(own: BlockState, world: &World, pos: BlockPos, tip: Direction) -> bool {
    let behind_pos = pos.relative(tip.opposite());
    let behind = world.block_state(behind_pos);
    world.is_face_sturdy(behind_pos, tip, SupportType::Full)
        || points(behind, tip) && behind.is_same_block(own)
}

/// `calculateTipDirection`：优先朝着玩家看的反方向。
fn tip_direction(
    own: BlockState,
    world: &World,
    pos: BlockPos,
    preferred: Direction,
) -> Option<Direction> {
    [preferred, preferred.opposite()]
        .into_iter()
        .find(|&tip| is_valid_placement(own, world, pos, tip))
}

/// `calculateSpeleothemThickness`。
fn thickness(
    own: BlockState,
    world: &World,
    pos: BlockPos,
    tip: Direction,
    merge_opposing_tips: bool,
) -> SpeleothemThickness {
    let base = tip.opposite();
    let in_front = world.block_state(pos.relative(tip));
    if points(in_front, base) && in_front.is_same_block(own) {
        let merged = in_front.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::TipMerge;
        return if merge_opposing_tips || merged {
            SpeleothemThickness::TipMerge
        } else {
            SpeleothemThickness::Tip
        };
    }
    if !points(in_front, tip) {
        return SpeleothemThickness::Tip;
    }
    match in_front.get(SPELEOTHEM_THICKNESS) {
        SpeleothemThickness::Tip | SpeleothemThickness::TipMerge => SpeleothemThickness::Frustum,
        _ if points(world.block_state(pos.relative(base)), tip) => SpeleothemThickness::Middle,
        _ => SpeleothemThickness::Base,
    }
}

/// `spawnFallingStalactite`：从 `pos` 往下到尖端的整根都掉下来。
fn drop_stalactite(state: BlockState, world: &mut World, pos: BlockPos) {
    let mut fall_pos = pos;
    let mut fall_state = state;
    while is_stalactite(fall_state) {
        fall(world, fall_pos, fall_state);
        if is_tip_or_merged(fall_state) {
            break;
        }
        fall_pos = fall_pos.below();
        fall_state = world.block_state(fall_pos);
    }
}

/// `findBlockVertical`。
fn find_block_vertical(
    world: &World,
    pos: BlockPos,
    direction: Direction,
    path: impl Fn(BlockPos, BlockState) -> bool,
    target: impl Fn(BlockState) -> bool,
) -> Option<BlockPos> {
    let mut current = pos;
    for _ in 1..MAX_SEARCH_LENGTH {
        current = current.relative(direction);
        let state = world.block_state(current);
        if target(state) {
            return Some(current);
        }
        if !world.is_inside_build_height(current.y) || !path(current, state) {
            return None;
        }
    }
    None
}

/// `canDripThrough`：水滴能穿过这一格。
fn can_drip_through(world: &World, pos: BlockPos, state: BlockState) -> bool {
    if state.is_air() {
        return true;
    }
    if state.is_solid_render() || !state.fluid().is_empty() {
        return false;
    }
    let collision = state.collision_boxes(world, pos, CollisionContext::Empty);
    !shape::join_is_not_empty(DRIP_SPACE, &collision, BooleanOp::And)
}

/// `findStalactiteTipAboveCauldron`。
pub(super) fn stalactite_tip_above(world: &World, cauldron: BlockPos) -> Option<BlockPos> {
    find_block_vertical(
        world,
        cauldron,
        Direction::Up,
        |pos, state| can_drip_through(world, pos, state),
        is_free_hanging_stalactite,
    )
}

/// `findRootBlock`：这根钟乳石上面第一个不是它的方块。
fn root_above(world: &World, pos: BlockPos, stalactite: BlockState) -> Option<BlockPos> {
    let tip = stalactite.get(VERTICAL_DIRECTION);
    let same_stalactite = |state: BlockState| state.is_same_block(stalactite);
    find_block_vertical(
        world,
        pos,
        tip.opposite(),
        |_, state| same_stalactite(state) && state.get(VERTICAL_DIRECTION) == tip,
        |state| !same_stalactite(state),
    )
}

/// `getCauldronFillFluidType`：这根钟乳石能往下滴的流体，主世界的泥巴也算水。
pub(super) fn cauldron_fill_fluid(world: &World, stalactite: BlockPos) -> Option<Fluid> {
    let state = world.block_state(stalactite);
    if !is_stalactite(state) {
        return None;
    }
    let above = root_above(world, stalactite, state)?.above();
    let fluid_above = world.fluid_state(above);
    if world.block_state(above).is(blocks::MUD) || fluid_above.is(fluid::WATER) {
        Some(Fluid::Water)
    } else if fluid_above.is(fluid::LAVA) {
        Some(Fluid::Lava)
    } else {
        None
    }
}

impl Behaviour for Speleothem {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let own = default_state(block);
        let preferred = ctx.nearest_looking_vertical_direction().opposite();
        let tip = tip_direction(own, world, pos, preferred)?;
        let thickness = thickness(own, world, pos, tip, !ctx.is_secondary_use_active());
        Some(
            own.with(VERTICAL_DIRECTION, tip)
                .with(SPELEOTHEM_THICKNESS, thickness)
                .with(WATERLOGGED, is_water_source(world, pos)),
        )
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        is_valid_placement(state, world, pos, state.get(VERTICAL_DIRECTION))
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        tick_water_if_waterlogged(state, world, pos);
        if direction.axis().is_horizontal() {
            return state;
        }
        let tip = state.get(VERTICAL_DIRECTION);
        if tip == Direction::Down && world.has_scheduled_tick(pos, state.block()) {
            return state;
        }
        if direction == tip.opposite() && !state.can_survive(world, pos) {
            let delay = if tip == Direction::Down {
                DELAY_BEFORE_FALLING
            } else {
                1
            };
            world.schedule_tick(pos, state.block(), delay);
            return state;
        }
        let merge = state.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::TipMerge;
        state.with(
            SPELEOTHEM_THICKNESS,
            thickness(state, world, pos, tip, merge),
        )
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if is_stalagmite(state) && !state.can_survive(world, pos) {
            world.destroy_block(pos, UPDATE_LIMIT);
        } else {
            drop_stalactite(state, world, pos);
        }
    }
}
