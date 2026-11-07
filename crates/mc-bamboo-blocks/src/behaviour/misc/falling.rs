//! `ConcretePowderBlock`、`DragonEggBlock`。
//!
//! 它们的父类 `FallingBlock` 排的计划刻只用来下落，下落不在范围里，这里都不排。
//! 没有别的覆盖的子类 `ColoredFallingBlock`、`SandBlock` 不用写结构体。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, SupportType},
    world::World,
};

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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `DragonEggBlock`。传送的目标格是随机的，第 1 章说随机分支取一个确定的值，这里留在原地。
pub(super) struct DragonEgg;

impl Behaviour for DragonEgg {
    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
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
