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
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, SupportType},
    world::World,
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

/// `DragonEggBlock`。传送的目标格是随机的，第 1 章说随机分支取一个确定的值，这里留在原地。
pub(super) struct DragonEgg;

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

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
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

    /// 官方的传送是随机的，oracle 比不了，这里固定下来：龙蛋留在原地，点击被吃掉。
    #[test]
    fn a_dragon_egg_stays_where_it_is() {
        let pos = BlockPos::new(0, 64, 0);
        let mut level = TestLevel::new();
        level.set(pos, BlockState(blocks::DRAGON_EGG));
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(pos));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(pos).is(blocks::DRAGON_EGG));
    }
}
