//! `VineBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::can_spread;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, multiface_can_attach_to, pipe_property},
    },
    context::PlaceContext,
    state::{BlockState, UP},
    world::{UPDATE_CLIENTS, World},
};

/// 藤蔓每次随机刻有四分之一的机会长。
const SPREAD_CHANCE: i32 = 4;
/// 前面是空气、两侧又没法绕时，搭到上面方块的概率。官方拿 `nextFloat()` 和 double 的 0.05 比。
const CLIMB_UP_CHANCE: f64 = 0.05;

/// `PROPERTY_BY_DIRECTION` 的方向：藤蔓没有朝下的面。
const FACES: [Direction; 5] = [
    Direction::Up,
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// `VineBlock`。
pub(super) struct Vine;

/// `countFaces`。
fn count_faces(state: BlockState) -> usize {
    FACES
        .into_iter()
        .filter(|&face| state.get(pipe_property(face)))
        .count()
}

/// `isAcceptableNeighbour`。
fn is_acceptable_neighbour(world: &World, neighbor_pos: BlockPos, direction: Direction) -> bool {
    multiface_can_attach_to(
        world,
        direction,
        neighbor_pos,
        world.block_state(neighbor_pos),
    )
}

/// `canSupportAtFace`：这一面贴着方块，或者上面的藤蔓在同一面垂下来。`vine` 是任意一个藤蔓状态。
fn can_support_at_face(vine: BlockState, world: &World, pos: BlockPos, face: Direction) -> bool {
    if face == Direction::Down {
        return false;
    }
    if is_acceptable_neighbour(world, pos.relative(face), face) {
        return true;
    }
    let above = world.block_state(pos.above());
    face.axis().is_horizontal() && above.is_same_block(vine) && above.get(pipe_property(face))
}

/// `getUpdatedState`：去掉已经贴不住的面。
fn updated_state(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    let mut updated = state;
    if updated.get(UP) {
        // 官方传的方向是朝下，看的是上面一格的顶面，这里保持一样。
        let attached = is_acceptable_neighbour(world, pos.above(), Direction::Down);
        updated = updated.with(UP, attached);
    }
    for face in Direction::HORIZONTAL {
        let property = pipe_property(face);
        if updated.get(property) {
            let attached = can_support_at_face(state, world, pos, face);
            updated = updated.with(property, attached);
        }
    }
    updated
}

impl Behaviour for Vine {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let default = default_state(block);
        let clicked = world.block_state(pos);
        let clicked_vine = clicked.is_same_block(default);
        let base = if clicked_vine { clicked } else { default };
        let free_face = ctx.nearest_looking_directions().into_iter().find(|&face| {
            face != Direction::Down
                && !(clicked_vine && clicked.get(pipe_property(face)))
                && can_support_at_face(default, world, pos, face)
        });
        match free_face {
            Some(face) => Some(base.with(pipe_property(face), true)),
            None => clicked_vine.then_some(base),
        }
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let clicked = world.block_state(ctx.clicked_pos());
        if clicked.is_same_block(state) {
            count_faces(clicked) < FACES.len()
        } else {
            Plain.can_be_replaced(state, ctx, world)
        }
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        count_faces(updated_state(state, world, pos)) > 0
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
        if direction == Direction::Down {
            return Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        }
        let updated = updated_state(state, world, pos);
        if count_faces(updated) > 0 {
            updated
        } else {
            BlockState::AIR
        }
    }

    /// 四分之一的机会挑一个方向：水平的面还空着时往那边爬，否则往上或者往下长。
    ///
    /// `VineBlock.randomTick` 先看 `GameRules.SPREAD_VINES`。关着时不调用 `nextInt`。
    ///
    /// `VineBlock.randomTick` checks `GameRules.SPREAD_VINES` first and does not call `nextInt` when it is off.
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !world.block_game_rules().spread_vines {
            return;
        }
        if world.random().next_int(SPREAD_CHANCE) != 0 {
            return;
        }
        let direction = world.random().direction();
        if direction.axis().is_horizontal() && !state.get(pipe_property(direction)) {
            if can_spread(state, world, pos) {
                spread_sideways(state, world, pos, direction);
            }
            return;
        }
        if direction == Direction::Up && pos.y < world.max_y() && grow_up(state, world, pos) {
            return;
        }
        if pos.y > world.min_y() {
            grow_down(state, world, pos);
        }
    }
}

/// 水平方向上那一面还空着：前面是空气时绕过拐角或者往上搭，前面是方块时贴上去。
fn spread_sideways(state: BlockState, world: &mut World, pos: BlockPos, direction: Direction) {
    let test_pos = pos.relative(direction);
    if world.block_state(test_pos).is_air() {
        spread_into_air(state, world, pos, direction);
    } else if is_acceptable_neighbour(world, test_pos, direction) {
        world.set_block(
            pos,
            state.with(pipe_property(direction), true),
            UPDATE_CLIENTS,
        );
    }
}

/// 前面是空气：两侧有面的话，先试着在前面那格贴着侧面的方块，再试着绕到侧面的拐角，
/// 最后很小的机会贴着前面一格上面的方块。
fn spread_into_air(state: BlockState, world: &mut World, pos: BlockPos, direction: Direction) {
    let test_pos = pos.relative(direction);
    let clockwise = direction.clockwise();
    let counter = direction.counter_clockwise();
    let has_clockwise = state.get(pipe_property(clockwise));
    let has_counter = state.get(pipe_property(counter));
    let clockwise_pos = test_pos.relative(clockwise);
    let counter_pos = test_pos.relative(counter);
    let fresh = state.default_state();
    let opposite = direction.opposite();
    if has_clockwise && is_acceptable_neighbour(world, clockwise_pos, clockwise) {
        world.set_block(
            test_pos,
            fresh.with(pipe_property(clockwise), true),
            UPDATE_CLIENTS,
        );
    } else if has_counter && is_acceptable_neighbour(world, counter_pos, counter) {
        world.set_block(
            test_pos,
            fresh.with(pipe_property(counter), true),
            UPDATE_CLIENTS,
        );
    } else if has_clockwise
        && world.is_empty_block(clockwise_pos)
        && is_acceptable_neighbour(world, pos.relative(clockwise), opposite)
    {
        world.set_block(
            clockwise_pos,
            fresh.with(pipe_property(opposite), true),
            UPDATE_CLIENTS,
        );
    } else if has_counter
        && world.is_empty_block(counter_pos)
        && is_acceptable_neighbour(world, pos.relative(counter), opposite)
    {
        world.set_block(
            counter_pos,
            fresh.with(pipe_property(opposite), true),
            UPDATE_CLIENTS,
        );
    } else if f64::from(world.random().next_float()) < CLIMB_UP_CHANCE
        && is_acceptable_neighbour(world, test_pos.above(), Direction::Up)
    {
        world.set_block(test_pos, fresh.with(UP, true), UPDATE_CLIENTS);
    }
}

/// 朝上：顶上贴得住就贴上去；上面是空气时往上长一格，每个水平面一半的机会留着。
/// 上面不是空气时返回假，官方接着试往下长；别的情况这次随机刻都做完了。
fn grow_up(state: BlockState, world: &mut World, pos: BlockPos) -> bool {
    if can_support_at_face(state, world, pos, Direction::Up) {
        world.set_block(pos, state.with(UP, true), UPDATE_CLIENTS);
        return true;
    }
    let above_pos = pos.above();
    if !world.is_empty_block(above_pos) {
        return false;
    }
    if !can_spread(state, world, pos) {
        return true;
    }
    let mut above = state;
    for face in Direction::HORIZONTAL {
        if world.random().next_boolean()
            || !is_acceptable_neighbour(world, above_pos.relative(face), face)
        {
            above = above.with(pipe_property(face), false);
        }
    }
    if has_horizontal_connection(above) {
        world.set_block(above_pos, above, UPDATE_CLIENTS);
    }
    true
}

/// 下面是空气或者藤蔓时，随机把一些面复制下去。
fn grow_down(state: BlockState, world: &mut World, pos: BlockPos) {
    let below_pos = pos.below();
    let below = world.block_state(below_pos);
    if !below.is_air() && !below.is_same_block(state) {
        return;
    }
    let before = if below.is_air() {
        state.default_state()
    } else {
        below
    };
    let after = copy_random_faces(state, before, world);
    if before != after && has_horizontal_connection(after) {
        world.set_block(below_pos, after, UPDATE_CLIENTS);
    }
}

/// `copyRandomFaces`：每个水平面一半的机会，`from` 有这一面时 `to` 也加上。
fn copy_random_faces(from: BlockState, to: BlockState, world: &World) -> BlockState {
    let mut copied = to;
    for face in Direction::HORIZONTAL {
        if world.random().next_boolean() {
            let property = pipe_property(face);
            if from.get(property) {
                copied = copied.with(property, true);
            }
        }
    }
    copied
}

/// `hasHorizontalConnection`。
fn has_horizontal_connection(state: BlockState) -> bool {
    Direction::HORIZONTAL
        .into_iter()
        .any(|face| state.get(pipe_property(face)))
}
