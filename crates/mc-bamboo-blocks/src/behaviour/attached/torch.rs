//! `WallTorchBlock`。`TorchBlock` 只用父类 `BaseTorchBlock` 的行为，在 `common` 里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, has_sturdy_wall, lost_wall_support, wall_state_for_placement},
    },
    context::PlaceContext,
    state::BlockState,
    world::World,
};

/// `WallTorchBlock`。官方继承 `TorchBlock`，放置、存活、形状更新都覆盖了，用不到父类。
pub(super) struct WallTorch;

impl Behaviour for WallTorch {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        wall_state_for_placement(default_state(block), ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_sturdy_wall(state, world, pos)
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
        if lost_wall_support(state, world, pos, direction) {
            BlockState::AIR
        } else {
            state
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        Effect, InteractionResult,
        behaviour::attached::test_support::hit_side,
        state::{HORIZONTAL_FACING, SLAB_TYPE, SlabType},
        testing::{TestLevel, actor_holding, break_block, use_item},
    };

    const WALL: BlockPos = BlockPos::new(0, 65, 0);
    const FLOOR: BlockPos = BlockPos::new(1, 64, 0);

    fn level_with_wall() -> TestLevel {
        let mut level = TestLevel::new();
        level.set(WALL, BlockState(blocks::STONE));
        level.set(FLOOR, BlockState(blocks::STONE));
        level
    }

    #[test]
    fn torch_on_the_floor_stands_up() {
        let mut level = level_with_wall();
        let outcome = use_item(
            &mut level,
            &actor_holding(items::TORCH),
            hit_side(FLOOR, Direction::Up),
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(FLOOR.above()), BlockState(blocks::TORCH));
    }

    #[test]
    fn torch_on_a_wall_becomes_a_wall_torch_facing_away() {
        let mut level = level_with_wall();
        use_item(
            &mut level,
            &actor_holding(items::TORCH),
            hit_side(WALL, Direction::East),
        );
        let placed = level.get(WALL.east());
        assert!(placed.is(blocks::WALL_TORCH));
        assert_eq!(placed.get(HORIZONTAL_FACING), Direction::East);
    }

    #[test]
    fn torch_under_a_ceiling_has_nothing_to_stand_on() {
        let mut level = TestLevel::new();
        let ceiling = BlockPos::new(0, 70, 0);
        level.set(ceiling, BlockState(blocks::STONE));
        let outcome = use_item(
            &mut level,
            &actor_holding(items::TORCH),
            hit_side(ceiling, Direction::Down),
        );
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(ceiling.below()).is_air());
    }

    #[test]
    fn torch_stands_on_the_centre_of_a_fence() {
        let mut level = TestLevel::new();
        let fence = BlockPos::new(0, 64, 0);
        level.set(fence, BlockState(blocks::OAK_FENCE));
        use_item(
            &mut level,
            &actor_holding(items::TORCH),
            hit_side(fence, Direction::Up),
        );
        assert_eq!(level.get(fence.above()), BlockState(blocks::TORCH));
    }

    #[test]
    fn torch_does_not_stand_on_a_bottom_slab() {
        let mut level = TestLevel::new();
        let slab = BlockPos::new(0, 64, 0);
        let bottom_slab = BlockState(blocks::OAK_SLAB).with(SLAB_TYPE, SlabType::Bottom);
        level.set(slab, bottom_slab);
        let outcome = use_item(
            &mut level,
            &actor_holding(items::TORCH),
            hit_side(slab, Direction::Up),
        );
        // 下半台阶的顶面不到方块顶，火把挂不上去。
        assert_eq!(outcome.result, InteractionResult::Fail);
    }

    #[test]
    fn losing_the_floor_breaks_the_torch() {
        let mut level = level_with_wall();
        let torch = FLOOR.above();
        use_item(
            &mut level,
            &actor_holding(items::TORCH),
            hit_side(FLOOR, Direction::Up),
        );

        let outcome = break_block(&mut level, FLOOR);
        assert!(level.get(FLOOR).is_air());
        assert!(level.get(torch).is_air());
        // 官方的顺序：先是玩家破坏的方块，再是连锁掉的火把。
        assert_eq!(
            outcome.effects,
            [
                Effect::Destroyed {
                    pos: FLOOR,
                    state: BlockState(blocks::STONE),
                    except_initiator: true,
                },
                Effect::Destroyed {
                    pos: torch,
                    state: BlockState(blocks::TORCH),
                    except_initiator: false,
                },
            ]
        );
    }

    #[test]
    fn losing_the_wall_breaks_only_the_torch_on_it() {
        let mut level = level_with_wall();
        // 这支火把站在地上，和墙只是挨着。
        let on_floor = FLOOR.above();
        let actor = actor_holding(items::TORCH);
        use_item(&mut level, &actor, hit_side(WALL, Direction::North));
        use_item(&mut level, &actor, hit_side(FLOOR, Direction::Up));
        assert!(level.get(WALL.north()).is(blocks::WALL_TORCH));
        assert!(level.get(on_floor).is(blocks::TORCH));

        break_block(&mut level, WALL);
        assert!(level.get(WALL.north()).is_air());
        assert!(level.get(on_floor).is(blocks::TORCH));
    }

    #[test]
    fn wall_torch_ignores_changes_on_other_sides() {
        let mut level = level_with_wall();
        let torch = WALL.west();
        use_item(
            &mut level,
            &actor_holding(items::TORCH),
            hit_side(WALL, Direction::West),
        );
        level.set(torch.west(), BlockState(blocks::STONE));
        break_block(&mut level, torch.west());
        assert!(level.get(torch).is(blocks::WALL_TORCH));
    }

    #[test]
    fn soul_torch_and_copper_torch_share_the_torch_rules() {
        for (item, wall) in [
            (items::SOUL_TORCH, blocks::SOUL_WALL_TORCH),
            (items::COPPER_TORCH, blocks::COPPER_WALL_TORCH),
        ] {
            let mut level = level_with_wall();
            use_item(
                &mut level,
                &actor_holding(item),
                hit_side(WALL, Direction::South),
            );
            assert!(level.get(WALL.south()).is(wall));
        }
    }
}
