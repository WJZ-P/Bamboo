//! `FallingBlock` 和它的子类 `ConcretePowderBlock`、`DragonEggBlock`，还有自己会掉的 `BrushableBlock`。
//!
//! 沙子和沙砾只覆盖了灰尘颜色，用 [`Falling`] 就够了。下面空着的时候，计划刻把这一格清空，
//! 换成 [`FallingBlockEntity`](crate::entity) 往下掉。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block, is_free},
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, SupportType},
    world::{UPDATE_CLIENTS, World},
};

/// `FallingBlock.getDelayAfterPlace()`。
pub(crate) const DELAY: i32 = 2;
/// `DragonEggBlock.getDelayAfterPlace()`。
const DRAGON_EGG_DELAY: i32 = 5;
/// `BrushableBlock` 的 `scheduleTick(pos, this, 2)`。
const BRUSHABLE_DELAY: i32 = 2;

/// `FallingBlock`。沙、沙砾这些只改了灰尘颜色的类用它。
pub(super) struct Falling;

/// `FallingBlock.onPlace` 和 `updateShape` 排的计划刻。
pub(crate) fn schedule_fall(state: BlockState, world: &mut World, pos: BlockPos, delay: i32) {
    world.schedule_tick(pos, state.block(), delay);
}

/// `FallingBlock.tick`：下面空着就掉下去，返回新实体的 ID 给 `falling` 钩子。
pub(crate) fn fall_if_free(state: BlockState, world: &mut World, pos: BlockPos) -> Option<i32> {
    let free = is_free(world.block_state(pos.below())) && pos.y >= world.min_y();
    free.then(|| world.spawn_falling_block(pos, state))
}

impl Behaviour for Falling {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        schedule_fall(state, world, pos, DELAY);
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
        schedule_fall(state, world, pos, DELAY);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        fall_if_free(state, world, pos);
    }
}

/// `BrushableBlock`。掉下去时不留掉落物，方块实体的重置不在范围里。
pub(super) struct Brushable;

impl Behaviour for Brushable {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        schedule_fall(state, world, pos, BRUSHABLE_DELAY);
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
        schedule_fall(state, world, pos, BRUSHABLE_DELAY);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if let Some(id) = fall_if_free(state, world, pos)
            && let Some(entity) = world.entity_mut(id)
        {
            entity.disable_drop();
        }
    }
}

/// `ConcretePowderBlock`。
pub(super) struct ConcretePowder;

/// `canSolidify`。
fn can_solidify(state: BlockState) -> bool {
    state.fluid().is_water()
}

/// `touchesLiquid`：旁边或者上面有水，而且水没有被完整的面挡着。
/// 下面的水只有这一格自己也有水时才算，这样粉末能落在水底的方块上。
fn touches_liquid(world: &World, pos: BlockPos) -> bool {
    let wet_here = can_solidify(world.block_state(pos));
    Direction::ALL
        .into_iter()
        .filter(|&side| side != Direction::Down || wet_here)
        .any(|side| {
            let neighbor = world.block_state(pos.relative(side));
            can_solidify(neighbor) && !neighbor.is_face_sturdy(side.opposite(), SupportType::Full)
        })
}

fn concrete_of(state: BlockState) -> BlockState {
    default_state(field_block(state.block(), "concrete"))
}

impl Behaviour for ConcretePowder {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let default = default_state(block);
        let solid = can_solidify(world.block_state(pos)) || touches_liquid(world, pos);
        Some(if solid { concrete_of(default) } else { default })
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        schedule_fall(state, world, pos, DELAY);
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
        if touches_liquid(world, pos) {
            return concrete_of(state);
        }
        schedule_fall(state, world, pos, DELAY);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        fall_if_free(state, world, pos);
    }
}

/// `DragonEggBlock`：右键时传送到附近随机一个下面有东西的空格。
pub(super) struct DragonEgg;

/// 传送最多试这么多次。
const TELEPORT_ATTEMPTS: i32 = 1000;

impl DragonEgg {
    /// `teleport`。传送的粒子（`levelEvent(2015)`）不做。
    fn teleport(state: BlockState, world: &mut World, pos: BlockPos) {
        for _ in 0..TELEPORT_ATTEMPTS {
            let random = world.random();
            let dx = random.next_int(16) - random.next_int(16);
            let dy = random.next_int(8) - random.next_int(8);
            let dz = random.next_int(16) - random.next_int(16);
            let target = pos.offset(dx, dy, dz);
            if world.block_state(target).is_air()
                && !world.block_state(target.below()).is_air()
                && world.is_within_world_border(target)
                && world.is_inside_build_height(target.y)
            {
                world.set_block(target, state, UPDATE_CLIENTS);
                world.remove_block(pos);
                return;
            }
        }
    }
}

impl Behaviour for DragonEgg {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        schedule_fall(state, world, pos, DRAGON_EGG_DELAY);
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
        schedule_fall(state, world, pos, DRAGON_EGG_DELAY);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        fall_if_free(state, world, pos);
    }

    /// `DragonEggBlock.attack`：拿手打一下和右键一样，传送到附近。
    ///
    /// `DragonEggBlock.attack`: punching teleports it, the same as a right click.
    fn attack(&self, state: BlockState, world: &mut World, pos: BlockPos, _player: &Actor) {
        Self::teleport(state, world, pos);
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Self::teleport(state, world, ctx.clicked_pos());
        InteractionResult::Success
    }
}

/// `Fallable.onLand`：落地之后方块已经放好了。
///
/// 其他类的覆盖只放音效、粒子，只有混凝土粉末会再改一次世界。
pub(crate) fn on_land(world: &mut World, pos: BlockPos, state: BlockState, replaced: BlockState) {
    if state.is_a(BlockAncestor::ConcretePowderBlock)
        && (can_solidify(replaced) || touches_liquid(world, pos))
    {
        world.set_block_and_update(pos, concrete_of(state));
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::testing::{TestLevel, actor_holding, top_hit, use_item};

    /// 只有一格能落脚时，龙蛋迟早传送到那里。
    #[test]
    fn a_dragon_egg_teleports_onto_a_block() {
        let pos = BlockPos::new(0, 64, 0);
        let landing = BlockPos::new(3, 64, -2);
        let mut level = TestLevel::new();
        level.set(pos, BlockState(blocks::DRAGON_EGG));
        level.set(pos.below(), BlockState(blocks::STONE));
        level.set(landing.below(), BlockState(blocks::STONE));
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(pos));
        assert_eq!(outcome.result, InteractionResult::Success);
        let egg = BlockState(blocks::DRAGON_EGG);
        let moved = level.get(landing) == egg && level.get(pos).is_air();
        let stayed = level.get(pos) == egg;
        assert!(moved || stayed);
    }
}
