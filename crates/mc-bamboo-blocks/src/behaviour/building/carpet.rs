//! `CarpetBlock`、`WoolCarpetBlock`、`MossyCarpetBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, multiface_can_attach_at},
    },
    context::{Actor, PlaceContext},
    state::{BOTTOM, BlockState, EAST_WALL, NORTH_WALL, Property, SOUTH_WALL, WEST_WALL, WallSide},
    world::World,
};

/// `CarpetBlock`。
pub(super) struct Carpet;

impl Behaviour for Carpet {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        !world.is_empty_block(pos.below())
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

/// `MossyCarpetBlock`：苍白苔毯，贴着墙时往墙上爬。
pub(super) struct MossyCarpet;

/// `getPropertyForFace`。
const fn side_property(side: Direction) -> Property<WallSide> {
    match side {
        Direction::North => NORTH_WALL,
        Direction::East => EAST_WALL,
        Direction::South => SOUTH_WALL,
        Direction::West => WEST_WALL,
        Direction::Up | Direction::Down => panic!("moss only climbs horizontal sides"),
    }
}

/// `hasFaces`。
fn has_faces(state: BlockState) -> bool {
    state.get(BOTTOM)
        || Direction::HORIZONTAL
            .into_iter()
            .any(|side| state.get(side_property(side)) != WallSide::None)
}

/// `getUpdatedState` 里一侧的结果。
fn updated_side(
    state: BlockState,
    world: &World,
    pos: BlockPos,
    side: Direction,
    create_sides: bool,
) -> WallSide {
    let property = side_property(side);
    let current = if !multiface_can_attach_at(world, pos, side) {
        WallSide::None
    } else if create_sides {
        WallSide::Low
    } else {
        state.get(property)
    };
    if current != WallSide::Low {
        return current;
    }

    let above = world.block_state(pos.above());
    let climbed_above = above.is(blocks::PALE_MOSS_CARPET)
        && above.get(property) != WallSide::None
        && !above.get(BOTTOM);
    let below = world.block_state(pos.below());
    let bare_below = !state.get(BOTTOM)
        && below.is(blocks::PALE_MOSS_CARPET)
        && below.get(property) == WallSide::None;
    if bare_below {
        WallSide::None
    } else if climbed_above {
        WallSide::Tall
    } else {
        WallSide::Low
    }
}

/// `getUpdatedState`。
fn updated_state(
    state: BlockState,
    world: &World,
    pos: BlockPos,
    create_sides: bool,
) -> BlockState {
    let create_sides = create_sides || state.get(BOTTOM);
    Direction::HORIZONTAL
        .into_iter()
        .fold(state, |updated, side| {
            let value = updated_side(updated, world, pos, side, create_sides);
            updated.with(side_property(side), value)
        })
}

/// `createTopperWithSideChance`：上面一格要长出的苔毯，长不出来时是空气。
///
/// 放下时每个有面的一侧抛一次硬币，反面就去掉；骨粉全部保留，`flip_coins` 是假。
fn topper(world: &World, pos: BlockPos, flip_coins: bool) -> BlockState {
    let above = pos.above();
    let previous = world.block_state(above);
    let carpet_above = previous.is(blocks::PALE_MOSS_CARPET);
    let blocked = if carpet_above {
        previous.get(BOTTOM)
    } else {
        !previous.is_replaceable()
    };
    if blocked {
        return BlockState::AIR;
    }
    let bare = BlockState(blocks::PALE_MOSS_CARPET).with(BOTTOM, false);
    let mut topper = updated_state(bare, world, above, true);
    for side in Direction::HORIZONTAL {
        let property = side_property(side);
        if topper.get(property) != WallSide::None && flip_coins && !world.random().next_boolean() {
            topper = topper.with(property, WallSide::None);
        }
    }
    if has_faces(topper) && topper != previous {
        topper
    } else {
        BlockState::AIR
    }
}

impl Behaviour for MossyCarpet {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Some(updated_state(
            default_state(block),
            world,
            ctx.clicked_pos(),
            true,
        ))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        if state.get(BOTTOM) {
            !below.is_air()
        } else {
            below.is_same_block(state) && below.get(BOTTOM)
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
        if !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        let updated = updated_state(state, world, pos, false);
        if has_faces(updated) {
            updated
        } else {
            BlockState::AIR
        }
    }

    fn set_placed_by(&self, _state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        let topper = topper(world, pos, true);
        if !topper.is_air() {
            world.set_block_and_update(pos.above(), topper);
        }
    }

    /// 只有底下那层能用骨粉：上面能长出苔毯。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        state.get(BOTTOM) && !topper(world, pos, false).is_air()
    }

    fn perform_bonemeal(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        let topper = topper(world, pos, false);
        if !topper.is_air() {
            world.set_block_and_update(pos.above(), topper);
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        testing::{actor_holding, break_block, use_item},
    };

    #[test]
    fn a_carpet_needs_something_underneath() {
        let mut level = level_with_floor();
        use_item(&mut level, &actor_holding(items::RED_CARPET), top_of(FLOOR));
        assert!(level.get(FLOOR.above()).is(blocks::RED_CARPET));
        break_block(&mut level, FLOOR);
        assert!(level.get(FLOOR.above()).is_air());
    }

    /// 贴着墙的一侧抛硬币，测试世界的种子 0 第一次抛出正面，上面一格长出苔毯。
    #[test]
    fn moss_carpet_grows_up_a_wall_when_the_coin_says_so() {
        let mut level = level_with_floor();
        let carpet = FLOOR.above();
        level.set(carpet.west(), BlockState(blocks::STONE));
        level.set(carpet.west().above(), BlockState(blocks::STONE));
        use_item(
            &mut level,
            &actor_holding(items::PALE_MOSS_CARPET),
            top_of(FLOOR),
        );
        assert_eq!(level.get(carpet).get(WEST_WALL), WallSide::Tall);
        let top = level.get(carpet.above());
        assert!(top.is(blocks::PALE_MOSS_CARPET));
        assert!(!top.get(BOTTOM));
        assert_eq!(top.get(WEST_WALL), WallSide::Low);
    }

    #[test]
    fn moss_carpet_grows_nothing_in_the_open() {
        let mut level = level_with_floor();
        use_item(
            &mut level,
            &actor_holding(items::PALE_MOSS_CARPET),
            top_of(FLOOR),
        );
        assert!(level.get(FLOOR.above().above()).is_air());
    }
}
