//! `MultifaceBlock`、`GlowLichenBlock`、`SculkVeinBlock`。发光地衣的骨粉蔓延在这里，
//! 幽匿脉络的蔓延靠幽匿催发体，不在范围里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, registries::fluid};

use crate::{
    behaviour::{
        Behaviour,
        common::{
            default_state, multiface_can_attach_at, multiface_can_attach_to, pipe_property,
            tick_water_if_waterlogged,
        },
    },
    context::PlaceContext,
    state::{BlockState, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// `MultifaceBlock`。
pub(super) struct Multiface;

/// `hasFace`。
fn has_face(state: BlockState, face: Direction) -> bool {
    state.get(pipe_property(face))
}

/// `hasAnyFace`。
fn has_any_face(state: BlockState) -> bool {
    Direction::ALL.into_iter().any(|face| has_face(state, face))
}

/// `removeFace`：最后一面也没了就是空气。
fn remove_face(state: BlockState, face: Direction) -> BlockState {
    let updated = state.with(pipe_property(face), false);
    if has_any_face(updated) {
        updated
    } else {
        BlockState::AIR
    }
}

/// `getStateForPlacement(oldState, level, placementPos, placementDirection)`。
fn state_on_face(
    default: BlockState,
    old: BlockState,
    world: &World,
    pos: BlockPos,
    face: Direction,
) -> Option<BlockState> {
    let same_block = old.is_same_block(default);
    if same_block && has_face(old, face) || !multiface_can_attach_at(world, pos, face) {
        return None;
    }
    let base = if same_block {
        old
    } else {
        default.with(WATERLOGGED, old.fluid().is(fluid::WATER))
    };
    Some(base.with(pipe_property(face), true))
}

impl Behaviour for Multiface {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let old = world.block_state(pos);
        ctx.nearest_looking_directions()
            .into_iter()
            .find_map(|face| state_on_face(default_state(block), old, world, pos, face))
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, _world: &World) -> bool {
        let has_vacant_face = Direction::ALL
            .into_iter()
            .any(|face| !has_face(state, face));
        ctx.item != state.block().item || has_vacant_face
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_any_face(state)
            && Direction::ALL
                .into_iter()
                .filter(|&face| has_face(state, face))
                .all(|face| multiface_can_attach_at(world, pos, face))
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
        tick_water_if_waterlogged(state, world, pos);
        if !has_any_face(state) {
            return BlockState::AIR;
        }
        if has_face(state, direction)
            && !multiface_can_attach_to(world, direction, neighbor_pos, neighbor)
        {
            remove_face(state, direction)
        } else {
            state
        }
    }

    /// 只有 `GlowLichenBlock` 能用骨粉：有一面能往某个方向蔓延。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Direction::ALL.into_iter().any(|face| {
            Direction::ALL
                .into_iter()
                .any(|spread| spread_target(state, world, pos, face.opposite(), spread).is_some())
        })
    }

    /// `spreadFromRandomFaceTowardRandomDirection`：面和方向都洗乱了试，蔓延出去一格就停。
    /// 每换一个面都重新洗一次方向，和官方的流一样按需取随机数。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        for from in world.random().shuffled_directions() {
            if !has_face(state, from) {
                continue;
            }
            for spread in world.random().shuffled_directions() {
                if let Some((target, face)) = spread_target(state, world, pos, from, spread)
                    && spread_to_face(state, world, target, face)
                {
                    return;
                }
            }
        }
    }
}

/// `MultifaceSpreader.DEFAULT_SPREAD_ORDER`：同一格换一面、同一平面的邻格、绕过拐角。
fn spread_positions(
    pos: BlockPos,
    spread: Direction,
    from: Direction,
) -> [(BlockPos, Direction); 3] {
    let beside = pos.relative(spread);
    [
        (pos, spread),
        (beside, from),
        (beside.relative(from), spread.opposite()),
    ]
}

/// `getSpreadFromFaceTowardDirection`：从 `from` 这一面朝 `spread` 方向能蔓延到的第一个落点。
fn spread_target(
    state: BlockState,
    world: &World,
    pos: BlockPos,
    from: Direction,
    spread: Direction,
) -> Option<(BlockPos, Direction)> {
    if spread.axis() == from.axis() || !has_face(state, from) || has_face(state, spread) {
        return None;
    }
    spread_positions(pos, spread, from)
        .into_iter()
        .find(|&(target, face)| can_spread_into(state, world, target, face))
}

/// `DefaultSpreaderConfig.canSpreadInto`：落点是空气、水源或者同种方块，那一面又贴得住。
fn can_spread_into(lichen: BlockState, world: &World, target: BlockPos, face: Direction) -> bool {
    let existing = world.block_state(target);
    let replaceable = existing.is_air()
        || existing.is_same_block(lichen)
        || existing.is(blocks::WATER) && existing.fluid().is_source();
    replaceable && state_on_face(lichen.default_state(), existing, world, target, face).is_some()
}

/// `spreadToFace`：在落点加上这一面，世界真的变了才算蔓延出去。
fn spread_to_face(
    lichen: BlockState,
    world: &mut World,
    target: BlockPos,
    face: Direction,
) -> bool {
    let old = world.block_state(target);
    state_on_face(lichen.default_state(), old, world, target, face)
        .is_some_and(|spread| world.set_block(target, spread, UPDATE_CLIENTS))
}
