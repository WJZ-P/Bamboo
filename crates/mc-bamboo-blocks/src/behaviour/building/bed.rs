//! `BedBlock`、`StrawBedBlock`，官方的共同父类是 `AbstractBedBlock`。
//!
//! 这个世界是白天的主世界：`BedRule` 不让睡，也不会爆炸，右键床只吃掉点击。
//! 主世界里 `destroyOnUse` 总是假，`BedBlock.destroyOnUse`、`StrawBedBlock.destroyOnUse` 走不到。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockClass};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    level::Effect,
    state::{BED_PART, BedPart, BlockState, HORIZONTAL_FACING, OCCUPIED},
    world::{UPDATE_ALL, UPDATE_SUPPRESS_DROPS, World},
};

/// `AbstractBedBlock`。
pub(super) struct Bed;

/// `getNeighbourDirection`：另一半在哪个方向。
fn neighbour_direction(part: BedPart, facing: Direction) -> Direction {
    if part == BedPart::Foot {
        facing
    } else {
        facing.opposite()
    }
}

impl Behaviour for Bed {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let facing = ctx.horizontal_direction();
        let head = ctx.clicked_pos().relative(facing);
        let free = world.block_state(head).can_be_replaced(ctx, world)
            && world.is_within_world_border(head);
        free.then(|| default_state(block).with(HORIZONTAL_FACING, facing))
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
        let part = state.get(BED_PART);
        if direction != neighbour_direction(part, state.get(HORIZONTAL_FACING)) {
            return Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        }
        if neighbor.is_same_block(state) && neighbor.get(BED_PART) != part {
            state.with(OCCUPIED, neighbor.get(OCCUPIED))
        } else {
            BlockState::AIR
        }
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        let head = pos.relative(state.get(HORIZONTAL_FACING));
        world.set_block_and_update(head, state.with(BED_PART, BedPart::Head));
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        if state.get(BED_PART) == BedPart::Foot {
            remove_head_silently(state, world, pos);
        }
        Plain.player_will_destroy(state, world, pos, player)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let part = state.get(BED_PART);
        let direction = neighbour_direction(part, state.get(HORIZONTAL_FACING));
        let other = world.block_state(ctx.clicked_pos().relative(direction));
        if other.is_same_block(state) && other.get(BED_PART) != part {
            InteractionResult::SuccessServer
        } else {
            InteractionResult::Consume
        }
    }

    /// `StrawBedBlock` 放的是 2014 号事件，不是破坏效果，这里没有东西要记。
    fn spawn_destroy_particles(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.class() != BlockClass::StrawBedBlock {
            Plain.spawn_destroy_particles(state, world, pos);
        }
    }
}

/// `playerWillDestroy` 里的那一段：破坏床尾时先静默地拿走床头，这样不会多掉一份。
fn remove_head_silently(foot: BlockState, world: &mut World, pos: BlockPos) {
    let head_pos = pos.relative(foot.get(HORIZONTAL_FACING));
    let head = world.block_state(head_pos);
    if !head.is_same_block(foot) || head.get(BED_PART) != BedPart::Head {
        return;
    }
    world.set_block(
        head_pos,
        BlockState::AIR,
        UPDATE_ALL | UPDATE_SUPPRESS_DROPS,
    );
    world.push_effect(Effect::Destroyed {
        pos: head_pos,
        state: head,
        except_initiator: true,
    });
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        testing::{actor_holding, break_block, use_item},
    };

    const FOOT: BlockPos = FLOOR.above();

    fn placed_bed() -> crate::testing::TestLevel {
        let mut level = level_with_floor();
        level.set(FLOOR.south(), BlockState(blocks::STONE));
        use_item(&mut level, &actor_holding(items::RED_BED), top_of(FLOOR));
        level
    }

    #[test]
    fn placing_a_bed_builds_both_halves_ahead_of_the_player() {
        let level = placed_bed();
        let foot = level.get(FOOT);
        let head = level.get(FOOT.south());
        assert!(foot.is(blocks::RED_BED));
        assert_eq!(foot.get(HORIZONTAL_FACING), Direction::South);
        assert_eq!(foot.get(BED_PART), BedPart::Foot);
        assert_eq!(head, foot.with(BED_PART, BedPart::Head));
    }

    /// oracle 的场景都在原点附近，碰不到世界边界。
    #[test]
    fn the_head_of_a_bed_stays_inside_the_world_border() {
        let mut actor = actor_holding(items::RED_BED);
        actor.yaw = 270.0;
        for (x, expected) in [
            (29_999_982, InteractionResult::Success),
            (29_999_983, InteractionResult::Fail),
        ] {
            let floor = BlockPos::new(x, 64, 0);
            let mut level = crate::testing::TestLevel::new();
            level.set(floor, BlockState(blocks::STONE));
            let outcome = use_item(&mut level, &actor, top_of(floor));
            assert_eq!(outcome.result, expected);
        }
    }

    #[test]
    fn right_clicking_a_whole_bed_only_swings_the_arm() {
        let mut level = placed_bed();
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_of(FOOT));
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert!(!level.get(FOOT).get(OCCUPIED));
    }

    #[test]
    fn breaking_either_half_removes_the_bed() {
        for broken in [FOOT, FOOT.south()] {
            let mut level = placed_bed();
            break_block(&mut level, broken);
            assert!(level.get(FOOT).is_air());
            assert!(level.get(FOOT.south()).is_air());
        }
    }

    #[test]
    fn straw_beds_leave_no_destroy_effect_when_a_neighbour_breaks_them() {
        let mut level = level_with_floor();
        level.set(FLOOR.south(), BlockState(blocks::STONE));
        use_item(&mut level, &actor_holding(items::STRAW_BED), top_of(FLOOR));
        let head = level.get(FOOT.south());
        let outcome = break_block(&mut level, FOOT.south());
        assert_eq!(
            outcome.effects,
            [Effect::Destroyed {
                pos: FOOT.south(),
                state: head,
                except_initiator: true,
            }]
        );
        assert!(level.get(FOOT).is_air());
    }
}
